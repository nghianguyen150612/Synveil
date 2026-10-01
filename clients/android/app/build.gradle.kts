plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    id("org.jetbrains.kotlin.kapt")
}

val releaseVersionName = providers.gradleProperty("synveilVersionName").orElse("0.1.0")
val releaseVersionCode = providers.gradleProperty("synveilVersionCode").map(String::toInt).orElse(1)
val releaseStoreFile = providers.gradleProperty("synveilReleaseStoreFile").orElse(providers.environmentVariable("SYNVEIL_RELEASE_STORE_FILE")).orNull
val releaseStorePassword = providers.gradleProperty("synveilReleaseStorePassword").orElse(providers.environmentVariable("SYNVEIL_RELEASE_STORE_PASSWORD")).orNull
val releaseKeyAlias = providers.gradleProperty("synveilReleaseKeyAlias").orElse(providers.environmentVariable("SYNVEIL_RELEASE_KEY_ALIAS")).orNull
val releaseKeyPassword = providers.gradleProperty("synveilReleaseKeyPassword").orElse(providers.environmentVariable("SYNVEIL_RELEASE_KEY_PASSWORD")).orNull
val hasExternalReleaseSigning = listOf(
    releaseStoreFile,
    releaseStorePassword,
    releaseKeyAlias,
    releaseKeyPassword,
).all { !it.isNullOrBlank() }

android {
    namespace = "com.synveil.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.synveil.android"
        minSdk = 35
        targetSdk = 36
        versionCode = releaseVersionCode.get()
        versionName = releaseVersionName.get()

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        vectorDrawables {
            useSupportLibrary = true
        }
    }

    if (hasExternalReleaseSigning) {
        signingConfigs {
            create("release") {
                storeFile = file(checkNotNull(releaseStoreFile))
                storePassword = checkNotNull(releaseStorePassword)
                keyAlias = checkNotNull(releaseKeyAlias)
                keyPassword = checkNotNull(releaseKeyPassword)
            }
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            if (hasExternalReleaseSigning) {
                signingConfig = signingConfigs.getByName("release")
            }
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }

    kotlinOptions {
        jvmTarget = "21"
    }

    buildFeatures {
        buildConfig = true
        compose = true
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

dependencies {
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.compose.navigation)
    implementation(libs.androidx.compose.material3)
    implementation(libs.kotlinx.coroutines.core)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.okhttp)
    implementation("androidx.room:room-runtime:2.8.0")
    implementation("androidx.room:room-ktx:2.8.0")
    implementation("androidx.work:work-runtime-ktx:2.10.5")

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)

    testImplementation(libs.junit)
    testImplementation(libs.okhttp.mockwebserver)
    testImplementation("androidx.room:room-runtime:2.8.0")
    testImplementation("androidx.work:work-runtime-ktx:2.10.5")
    kapt("androidx.room:room-compiler:2.8.0")

    androidTestImplementation(libs.androidx.test.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.core)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.work:work-testing:2.10.5")
}
