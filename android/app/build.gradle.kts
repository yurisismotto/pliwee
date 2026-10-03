import com.google.protobuf.gradle.id
import com.google.protobuf.gradle.proto

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.protobuf)
}

// ---------------------------------------------------------------------------
// Release signing — ADR-0019.
//
// This file says HOW a release is signed; it never holds the key or its
// password. Both arrive from the environment, set for one Gradle run by
// `android/signing/build-release-bundle.sh` after a hidden prompt:
//
//   PLIWEE_UPLOAD_KEYSTORE           path to the upload PKCS#12 keystore,
//                                    which must live outside this repository
//   PLIWEE_UPLOAD_KEYSTORE_PASSWORD  its password
//
// A release packaging task with either missing FAILS. There is no fallback to
// the debug key and no silent unsigned artifact. The one escape hatch is
// explicit — `-Ppliwee.release.unsigned=true` — for checking R8 output
// where no key exists (CI); what it produces is labelled as not a production
// artifact, and Play refuses an unsigned bundle anyway.
// ---------------------------------------------------------------------------
val uploadKeystorePath: String? =
    providers.environmentVariable("PLIWEE_UPLOAD_KEYSTORE").orNull?.takeIf { it.isNotBlank() }
val uploadKeystorePassword: String? =
    providers.environmentVariable("PLIWEE_UPLOAD_KEYSTORE_PASSWORD").orNull?.takeIf { it.isNotEmpty() }
val allowUnsignedRelease: Boolean =
    providers.gradleProperty("pliwee.release.unsigned").orNull == "true"
val repositoryRoot: File = rootDir.parentFile.canonicalFile
val releaseSigningProblem: String? = when {
    uploadKeystorePath == null -> "PLIWEE_UPLOAD_KEYSTORE is not set"
    uploadKeystorePassword == null -> "PLIWEE_UPLOAD_KEYSTORE_PASSWORD is not set"
    !File(uploadKeystorePath).isFile -> "PLIWEE_UPLOAD_KEYSTORE does not name a file: $uploadKeystorePath"
    File(uploadKeystorePath).canonicalFile.startsWith(repositoryRoot) ->
        "the upload keystore is inside the repository ($uploadKeystorePath); it must live outside it"
    else -> null
}

android {
    namespace = "io.github.yurisismotto.pliwee"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.github.yurisismotto.pliwee"
        // API 29 (Android 10) is the floor: it is where TLS 1.3 is enabled by
        // default and where SSLParameters.setApplicationProtocols (ALPN)
        // became available. Below that we could not speak the protocol at all.
        minSdk = 29
        targetSdk = 36
        // versionName is the public semantic release version and follows
        // Pliwee's. versionCode is Play's ordering key: it must rise for
        // every upload to any Play track, and a code Play has seen once can
        // never be reused — not even for a bundle that was rejected.
        versionCode = 2
        versionName = "1.1.1"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    signingConfigs {
        if (releaseSigningProblem == null) {
            create("upload") {
                storeFile = file(uploadKeystorePath!!)
                storeType = "pkcs12"
                storePassword = uploadKeystorePassword
                keyAlias = "pliwee-upload"
                // PKCS#12 has one password for the store and its keys.
                keyPassword = uploadKeystorePassword
            }
        }
    }

    buildTypes {
        release {
            // Null when no upload key was supplied: the guard below then stops
            // any release packaging task before it runs.
            signingConfig = signingConfigs.findByName("upload")
            isMinifyEnabled = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }

    testOptions {
        unitTests {
            // `android.util.Log` is a stub on the unit-test classpath and
            // throws by default. Returning a default instead lets production
            // code keep its logging while the domain logic is tested on the
            // JVM. It is not a licence to test Android behaviour here: where
            // the platform's own behaviour is the subject — ClipboardManager,
            // the Keystore — the test is instrumented and runs on a device.
            isReturnDefaultValues = true
        }
    }

    sourceSets {
        getByName("main") {
            // Single source of truth: the same .proto files the Rust daemon
            // compiles. Neither side can drift from the other.
            proto { srcDir("../../protocol/proto") }
        }
        getByName("test") {
            // Same idea for the cross-language fixtures: the unit tests read
            // the very certificates the Rust suite reads, so the two
            // implementations cannot quietly disagree about what an identity
            // fingerprint is. Regenerate with:
            //   cargo run -p pliwee-core --example gen_test_vectors
            resources.srcDir("../../protocol/testdata")
            // Same idea for the design tokens: DesignTokensTest reads the very
            // file the desktop's own token test reads, so the two front ends
            // cannot quietly disagree about what "connected teal" is.
            resources.srcDir("../../docs/design")
        }
    }
}

protobuf {
    protoc { artifact = libs.protobuf.protoc.get().toString() }
    generateProtoTasks {
        all().forEach { task ->
            task.builtins {
                // "lite" keeps the generated code small and reflection-free,
                // which matters for an app that must stay tiny and start fast.
                id("java") { option("lite") }
            }
        }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.service)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.protobuf.javalite)
    implementation(libs.zxing.embedded)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.json)

    // Instrumented tests. The Keystore regression can only be proved on a
    // real device: the whole failure was the TEE refusing an operation, and
    // no JVM stand-in has a TEE to refuse it.
    androidTestImplementation(libs.junit)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.test.runner)
    // The notification consent screens are this wave's deliverable, and the
    // properties that matter about them — a switch that is off by default,
    // "Select all" never happening on its own, a status that never claims to
    // be mirroring while a gate is shut — are behaviour, not pixels.
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
}

// The release-signing guard. Decided when the task graph is known, so debug
// builds and unit tests never need a key, and any task that would *produce* a
// release APK or bundle cannot run without one.
val releaseArtifactTasks = setOf(
    "assembleRelease", "bundleRelease", "packageRelease",
    "packageReleaseBundle", "signReleaseBundle", "installRelease",
)
gradle.taskGraph.whenReady {
    val requested = allTasks.filter { it.project == project && it.name in releaseArtifactTasks }
    if (requested.isEmpty() || releaseSigningProblem == null) return@whenReady
    if (allowUnsignedRelease) {
        logger.warn(
            "PLIWEE: building an UNSIGNED release (-Ppliwee.release.unsigned=true). " +
                "This is NOT a production artifact and must never be uploaded.",
        )
        return@whenReady
    }
    throw GradleException(
        "Pliwee release signing is not configured: $releaseSigningProblem.\n" +
            "Requested: ${requested.joinToString { it.path }}.\n" +
            "Build a production release with android/signing/build-release-bundle.sh " +
            "(ADR-0019). There is no fallback to debug signing.",
    )
}

