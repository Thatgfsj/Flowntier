// Top-level build file. Plugin versions are declared here with
// `apply false` so the versions are centralized in
// `gradle/libs.versions.toml` and the modules opt in via
// `alias(libs.plugins.xxx)` (no version string in module files).
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.android) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
}

tasks.register("clean", Delete::class) {
    delete(rootProject.layout.buildDirectory)
}
