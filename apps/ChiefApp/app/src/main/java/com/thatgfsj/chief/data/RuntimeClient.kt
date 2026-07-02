package com.thatgfsj.chief.data

import com.thatgfsj.chief.tarot.TarotDrawResponse
import com.thatgfsj.chief.tarot.TarotListResponse
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

/**
 * Talks to the Flwntier desktop runtime over the JSON-RPC
 * 2.0 bridge on 127.0.0.1:8765 (or whatever LAN IP the
 * chairman entered). The runtime is loopback / LAN only —
 * no auth, just cleartext HTTP. v0.2 will add bearer tokens
 * before this app ever leaves the chairman's network.
 *
 * Three endpoints we call:
 *   GET /api/tarot/draw           → single card
 *   GET /api/tarot/draw?spread=3  → past/present/future
 *   GET /api/tarot/list           → 78-card deck metadata
 *   GET /health                   → for the settings pane
 */
class RuntimeClient(
    private val baseUrl: String = "http://127.0.0.1:8765",
) {
    private val client = OkHttpClient.Builder()
        .connectTimeout(5, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .writeTimeout(30, TimeUnit.SECONDS)
        .build()
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
    private val jsonMedia = "application/json; charset=utf-8".toMediaType()

    suspend fun ping(): Boolean = withContext(Dispatchers.IO) {
        rpcCall("GET", "/health").isSuccess
    }

    /**
     * Draw a single card. position is a free-form string the
     * caller can use to label the card (default: "抽卡").
     */
    suspend fun drawOne(position: String = "抽卡"): TarotDrawResponse? =
        withContext(Dispatchers.IO) {
            try {
                val resp = rpcCall("GET", "/api/tarot/draw")
                if (!resp.isSuccess) return@withContext null
                json.decodeFromString<TarotDrawResponse>(resp.body)
            } catch (e: Exception) {
                null
            }
        }

    /**
     * Draw a 3-card spread (past / present / future).
     */
    suspend fun drawThree(): TarotDrawResponse? =
        withContext(Dispatchers.IO) {
            try {
                val resp = rpcCall("GET", "/api/tarot/draw?spread=3")
                if (!resp.isSuccess) return@withContext null
                json.decodeFromString<TarotDrawResponse>(resp.body)
            } catch (e: Exception) {
                null
            }
        }

    /** Full 78-card deck metadata. */
    suspend fun listDeck(): TarotListResponse? =
        withContext(Dispatchers.IO) {
            try {
                val resp = rpcCall("GET", "/api/tarot/list")
                if (!resp.isSuccess) return@withContext null
                json.decodeFromString<TarotListResponse>(resp.body)
            } catch (e: Exception) {
                null
            }
        }

    private data class RpcResult(
        val isSuccess: Boolean,
        val body: String,
    )

    private fun rpcCall(method: String, path: String): RpcResult {
        val body = buildJsonObject {
            put("jsonrpc", "2.0")
            put("id", 1)
            put("method", method)
            put("path", path)
        }
        val req = Request.Builder()
            .url("$baseUrl/rpc")
            .post(body.toString().toRequestBody(jsonMedia))
            .build()
        client.newCall(req).execute().use { resp ->
            val raw = resp.body?.string().orEmpty()
            // Strip the JSON-RPC envelope and surface the inner
            // body so callers can decode the typed result.
            val inner: String = runCatching {
                val outer = json.parseToJsonElement(raw).jsonObject
                outer["result"]?.jsonObject?.get("body")?.toString() ?: raw
            }.getOrDefault(raw)
            return RpcResult(resp.isSuccessful, inner)
        }
    }
}

private val kotlinx.serialization.json.JsonElement.jsonObject
    get() = this as JsonObject
