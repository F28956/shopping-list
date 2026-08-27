import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.ksp)
}

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
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"
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
    testImplementation(libs.robolectric)
}
