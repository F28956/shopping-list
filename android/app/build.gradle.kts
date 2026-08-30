import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ksp)
}

// The units and aisles a device with no server still needs, copied from the file the
// server's own test guards against the migrations -- see `domain::reference`. Copied
// rather than kept as a second copy in this tree, because a second copy is a thing that
// drifts.
val copyReference by tasks.registering(Copy::class) {
    description = "Copies reference/reference.json into the APK's assets."
    from("${projectDir}/../../reference/reference.json")
    into("${projectDir}/src/main/assets")
}

tasks.matching { it.name.startsWith("merge") && it.name.endsWith("Assets") }
    .configureEach { dependsOn(copyReference) }

// The shared parser, compiled from Rust into the jniLibs this APK packages.
//
// A task rather than a note in the README, because "remember to run the script" means
// the day somebody changes the parser, rebuilds, and is served a stale answer by a
// library nobody rebuilt. Cargo is already incremental; when nothing changed this costs
// a fraction of a second.
val buildParser by tasks.registering(Exec::class) {
    description = "Compiles web/parsing into app/src/main/jniLibs."
    commandLine("${projectDir}/../scripts/build-parser.sh")
    // The source it actually depends on, so Gradle can skip the call entirely when
    // none of it has changed. Cargo would work it out too, but not before paying for
    // a process launch on every build.
    inputs.dir("${projectDir}/../../web/parsing")
    inputs.dir("${projectDir}/../../web/quickadd-ffi")
    outputs.dir("${projectDir}/src/main/jniLibs")
}

// The device's own server. Its own task rather than a line in `buildParser`, because
// the two are rebuilt for different reasons: the parser moves when a unit or a phrase
// does, and this moves whenever `domain` does -- which is most of the time.
val buildEmbedded by tasks.registering(Exec::class) {
    description = "Compiles web/embedded into app/src/main/jniLibs."
    commandLine("${projectDir}/../scripts/build-embedded.sh")
    inputs.dir("${projectDir}/../../web/embedded")
    inputs.dir("${projectDir}/../../web/domain")
    outputs.dir("${projectDir}/src/main/jniLibs")
}

tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }
    .configureEach { dependsOn(buildParser, buildEmbedded) }

android {
    testOptions {
        unitTests {
            isIncludeAndroidResources = true
        }
    }

    namespace = "com.cernauskas.shoppinglist"
    compileSdk = 36

    defaultConfig {
        // The same identifier as the phone and Mac apps. Google's Android OAuth
        // client is registered against it and the signing certificate's SHA-1.
        applicationId = "com.cernauskas.shoppinglist"
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        targetSdk = 36
        // Not 1 for ever. Android refuses to install a build whose versionCode is
        // not greater than the one already there, and the refusal says nothing about
        // versions -- so a sideloaded update simply fails. The count of commits
        // increases on its own and is the same number the Apple builds use; see
        // release/common.sh, which has the same note and the same escape hatch.
        versionCode = buildNumber()
        versionName = "0.1"
    }

    // Signing, if there is anything to sign with.
    //
    // Conditional rather than required, because every other thing this project builds
    // -- debug installs, unit tests, instrumented tests, CI -- works without a
    // keystore, and a build file that refused to configure without one would break
    // all of them to serve a release nobody is cutting today.
    //
    // What is *not* conditional is release/android.sh, which refuses to hand you an
    // unsigned APK. Silence here, loudness there.
    signingConfigs {
        keystoreProperties()?.let { held ->
            create("release") {
                storeFile = rootProject.file(held.getProperty("storeFile"))
                storePassword = held.getProperty("storePassword")
                keyAlias = held.getProperty("keyAlias")
                keyPassword = held.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        debug {
            // 10.0.2.2 is the host machine as seen from an emulator. Only debug
            // builds may talk to it, and only in the clear -- see
            // res/xml/network_security_config.xml.
            buildConfigField("String", "API_BASE_URL", "\"http://10.0.2.2:8080\"")
            buildConfigField("String", "GOOGLE_WEB_CLIENT_ID", "\"${googleWebClientId()}\"")
        }
        release {
            // Null where there is no keystore, which is what leaves `assembleRelease`
            // producing an unsigned APK rather than failing to configure.
            signingConfig = signingConfigs.findByName("release")
            isMinifyEnabled = false
            buildConfigField("String", "API_BASE_URL", "\"https://example.invalid\"")
            buildConfigField("String", "GOOGLE_WEB_CLIENT_ID", "\"${googleWebClientId()}\"")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // 17 rather than a toolchain: a toolchain names a JDK Gradle must go and find,
    // and the only ones here are 21 and 25. This compiles to 17 with the JDK it is
    // already running on.
    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }


    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets["main"].java.srcDirs("src/main/kotlin")
    // Instrumented tests, which the device's own server needs: `libembedded.so` is
    // compiled for an Android ABI, so a test on the build machine cannot load it.
    sourceSets["androidTest"].java.srcDirs("src/androidTest/kotlin")
}

// The schema Room generated, committed rather than regenerated. A migration is written
// against a schema that existed; without the file there is nothing to write it
// against, and nothing to notice when a change needs one.
ksp { arg("room.schemaLocation", "$projectDir/schemas") }

/// The web client id, which is what an Android ID token is addressed to.
///
/// Read from `local.properties` rather than committed: it is not a secret -- it is
/// embedded in every copy of the app -- but it names somebody's Google project, and a
/// checkout should not silently authenticate against mine.
/**
 * The keystore, or null.
 *
 * `keystore.properties` is gitignored and personal, exactly as `local.properties` is.
 * A missing file is not an error here -- see the note on `signingConfigs`.
 *
 * The file it names must be kept for ever. Android identifies an app by its package
 * *and its signing certificate*, so signing an update with a different key does not
 * produce an update: it produces an app the device refuses to install over the old
 * one, and the way out is for every person who has it to uninstall first -- which on
 * this app means throwing away a device that may be holding lists no server has heard
 * of. There is no recovery from losing this that does not cost somebody their data.
 *
 * The certificate's SHA-1 is also registered with Google against this package name,
 * so a new key means sign-in stops working until the console is told. See the note on
 * `applicationId`.
 */
fun keystoreProperties(): Properties? {
    val file = rootProject.file("keystore.properties")
    if (!file.exists()) return null
    val properties = Properties()
    file.inputStream().use(properties::load)
    // All four or none. A file with three of them configures a signing config that
    // fails deep inside the packaging task, saying something about a null password.
    val missing = listOf("storeFile", "storePassword", "keyAlias", "keyPassword")
        .filter { properties.getProperty(it).isNullOrBlank() }
    require(missing.isEmpty()) { "keystore.properties is missing: ${missing.joinToString(", ")}" }
    return properties
}

/**
 * The build number, shared with the Apple builds and derived the same way.
 *
 * Falls back to 1 where git cannot answer -- a source zip with no history, say --
 * rather than failing the build. That is a number that cannot be installed over
 * anything, which is the safe direction to be wrong in.
 */
fun buildNumber(): Int {
    System.getenv("BUILD_NUMBER")?.toIntOrNull()?.let { return it }
    return try {
        val process = ProcessBuilder("git", "rev-list", "--count", "HEAD")
            .directory(rootProject.projectDir)
            .start()
        val counted = process.inputStream.bufferedReader().readText().trim()
        process.waitFor()
        counted.toIntOrNull() ?: 1
    } catch (e: Exception) {
        1
    }
}

fun googleWebClientId(): String {
    val properties = Properties()
    val file = rootProject.file("local.properties")
    if (file.exists()) {
        file.inputStream().use(properties::load)
    }
    return properties.getProperty("googleWebClientId") ?: ""
}

dependencies {
    implementation(libs.androidx.room.runtime)
    implementation(libs.androidx.room.ktx)
    ksp(libs.androidx.room.compiler)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.ui)
    implementation(libs.androidx.ui.graphics)
    implementation(libs.androidx.ui.tooling.preview)
    implementation(libs.androidx.material3)
    implementation(libs.androidx.material.icons.extended)
    implementation(libs.androidx.navigation.compose)

    implementation(libs.okhttp)
    implementation(libs.okhttp.sse)
    implementation(libs.kotlinx.serialization.json)

    implementation(libs.androidx.credentials)
    implementation(libs.androidx.credentials.play.services)
    implementation(libs.googleid)

    debugImplementation(libs.androidx.ui.tooling)
    testImplementation(libs.junit)
    androidTestImplementation(libs.junit)
    androidTestImplementation(libs.androidx.test.junit)
    androidTestImplementation(libs.androidx.test.runner)
    testImplementation(libs.robolectric)
}
