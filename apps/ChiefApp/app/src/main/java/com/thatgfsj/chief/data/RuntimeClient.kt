package com.thatgfsj.chief.data

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/**
 * Talks to the Flowntier desktop runtime over HTTP. The
 * runtime exposes a JSON-RPC 2.0 bridge on a TCP port
 * (default 127.0.0.1:8765). For the Android chief app to
 * talk to a runtime on the same LAN, the chairman runs the
 * desktop with `FLOWNTIER_HTTP_BRIDGE=0.0.0.0:8765` (or
 * whatever port) and enters the host's LAN IP in the chief
 * app's settings.
 *
 * Two RPCs we call:
 *  - POST /rpc { "method": "POST",
 *                 "params": { "path": "/api/run_workflow",
 *                             "body": { "task": "..." } } }
 *    → returns wf_id immediately (the orchestrator runs
 *      on a tokio::spawn background task; event 000069).
 *  - POST /rpc { "method": "GET",
 *                 "params": { "path":
 *                             "/api/workflow/{wf_id}/status" } }
 *    → returns { status, phase, tasks_done, tasks_total,
 *                summary }. Polled every 2s while a workflow
 *                is running.
 *
 * The runtime is NOT authenticated — it's loopback / LAN
 * only. If the chairman runs it on a public interface
 * they're on their own (this is the v0.1.0 "chief app for
 * my own LAN" stance; a v0.2 will add bearer tokens).
 */
class RuntimeClient(
    private val baseUrl: String = "http://127.0.0.1:8765",
) {
    private val client = OkHttpClient.Builder()
        .connectTimeout(5, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .writeTimeout(30, TimeUnit.SECONDS)
        .build()
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }
    private val jsonMedia = "application/json; charset=utf-8".toMediaType()

    /**
     * POST /api/run_workflow. Returns the new wf_id in ~50ms
     * (the orchestrator runs async on the runtime side).
     */
    suspend fun startWorkflow(task: String): WorkflowStart =
        withContext(Dispatchers.IO) {
            val body = buildJsonObject {
                put("jsonrpc", "2.0")
                put("id", 1)
                put("method", "POST")
                put("params", buildJsonObject {
                    put("path", "/api/run_workflow")
                    put("body", buildJsonObject { put("task", task) })
                })
            }
            val req = Request.Builder()
                .url("$baseUrl/rpc")
                .post(body.toString().toRequestBody(jsonMedia))
                .build()
            client.newCall(req).execute().use { resp ->
                val raw = resp.body?.string().orEmpty()
                val parsed = json.parseToJsonElement(raw).jsonObject
                val result = parsed["result"]?.jsonObject
                    ?: error("RPC failed: ${parsed["error"]}")
                val body = result["body"]?.jsonObject
                    ?: error("RPC returned no body: $raw")
                WorkflowStart(
                    wfId = body["wf_id"]?.jsonPrimitive?.content
                        ?: error("no wf_id in response: $raw"),
                    status = body["status"]?.jsonPrimitive?.content ?: "running",
                )
            }
        }

    /**
     * POST /rpc with method=GET for /api/workflow/{wf_id}/status.
     * Returns parsed status JSON. Returns null on transport
     * error so the UI can keep polling.
     */
    suspend fun getStatus(wfId: String): WorkflowStatus? =
        withContext(Dispatchers.IO) {
            val body = buildJsonObject {
                put("jsonrpc", "2.0")
                put("id", 2)
                put("method", "GET")
                put("path", "/api/workflow/$wfId/status")
            }
            val req = Request.Builder()
                .url("$baseUrl/rpc")
                .post(body.toString().toRequestBody(jsonMedia))
                .build()
            try {
                client.newCall(req).execute().use { resp ->
                    val raw = resp.body?.string().orEmpty()
                    if (!resp.isSuccessful) return@withContext null
                    val parsed = json.parseToJsonElement(raw).jsonObject
                    val result = parsed["result"]?.jsonObject
                        ?: return@withContext null
                    val body = result["body"]?.jsonObject
                        ?: return@withContext null
                    WorkflowStatus(
                        wfId = body["wf_id"]?.jsonPrimitive?.content ?: wfId,
                        status = body["status"]?.jsonPrimitive?.content ?: "unknown",
                        phase = body["phase"]?.jsonPrimitive?.content ?: "unknown",
                        summary = body["summary"]?.jsonPrimitive?.contentOrNull,
                        tasksDone = body["tasks_done"]?.jsonPrimitive?.int ?: 0,
                        tasksTotal = body["tasks_total"]?.jsonPrimitive?.int ?: 0,
                    )
                }
            } catch (e: Exception) {
                null
            }
        }

    /**
     * Health check. Returns true if the runtime is reachable
     * and the JSON-RPC roundtrip succeeds. Used by the
     * settings pane to verify the host:port the chairman
     * entered is correct before they hit "send".
     */
    suspend fun ping(): Boolean = withContext(Dispatchers.IO) {
        val body = buildJsonObject {
            put("jsonrpc", "2.0")
            put("id", 0)
            put("method", "GET")
            put("path", "/health")
        }
        val req = Request.Builder()
            .url("$baseUrl/rpc")
            .post(body.toString().toRequestBody(jsonMedia))
            .build()
        try {
            client.newCall(req).execute().use { it.isSuccessful }
        } catch (e: Exception) {
            false
        }
    }
}

@Serializable
data class WorkflowStart(
    val wfId: String,
    val status: String,
)

@Serializable
data class WorkflowStatus(
    val wfId: String,
    val status: String,
    val phase: String,
    val summary: String? = null,
    val tasksDone: Int = 0,
    val tasksTotal: Int = 0,
)

private val JsonElement.jsonPrimitive: JsonPrimitive
    get() = this as JsonPrimitive
private val JsonElement.jsonObject: JsonObject
    get() = this as JsonObject
private val JsonPrimitive.contentOrNull: String?
    get() = if (isString) content else null
private val JsonPrimitive.int: Int
    get() = content.toIntOrNull() ?: 0
