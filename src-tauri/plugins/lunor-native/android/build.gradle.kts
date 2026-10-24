import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.lunorx.nativeplugin"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_17
    }
}

// rustls-platform-verifier: the Rust crate verifies TLS through this Kotlin component (see src/platform/android_tls.rs).
// The Gradle version is read from Cargo.lock so it always matches the crate. Every module needs the repository,
// because the app resolves the dependency at runtime, not only this library.
abstract class RustlsVersion : ValueSource<String, RustlsVersion.Params> {
    interface Params : ValueSourceParameters {
        val lockFile: RegularFileProperty
    }

    override fun obtain(): String {
        val lines = parameters.lockFile.get().asFile.readLines()
        val name = lines.indexOfFirst { it.trim() == "name = \"rustls-platform-verifier-android\"" }
        if (name < 0) error("rustls-platform-verifier-android not found in Cargo.lock")
        return lines.drop(name + 1).first { it.trimStart().startsWith("version = ") }.substringAfter('"').substringBefore('"')
    }
}

val rustlsVersion = providers.of(RustlsVersion::class.java) {
    parameters.lockFile.set(layout.projectDirectory.file("../../../Cargo.lock"))
}

rootProject.allprojects {
    repositories {
        maven { url = uri("https://github.com/rustls/rustls-platform-verifier/raw/maven-archive/android-release-support/maven/") }
    }
    configurations.configureEach {
        resolutionStrategy.eachDependency {
            if (requested.group == "org.rustls" && requested.name == "rustls-platform-verifier") {
                useVersion(rustlsVersion.get())
            }
        }
    }
}

dependencies {
    implementation("org.rustls:rustls-platform-verifier")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.media3:media3-exoplayer:1.4.1")
    implementation("androidx.media3:media3-session:1.4.1")
    implementation(project(":tauri-android"))
}
