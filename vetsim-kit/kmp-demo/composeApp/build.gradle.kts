// VetTwins : démo Kotlin Multiplatform du moteur vetsim (Android + bureau JVM).
// Le moteur Rust (crates/vetsim-ffi) est compilé par cargo, ses liaisons Kotlin sont générées
// par UniFFI, et les modules JSON (../modules) sont embarqués dans une source Kotlin générée.
// Usage pédagogique uniquement.

import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("org.jetbrains.kotlin.multiplatform")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.compose")
    id("com.android.application")
}

val kitDir: File = rootDir.parentFile
val cargo: String = listOf(System.getenv("CARGO"), "${System.getProperty("user.home")}/.cargo/bin/cargo")
    .firstOrNull { it != null && File(it).canExecute() } ?: "cargo"
val genDir = layout.buildDirectory.dir("generated/vetsim")

// Bibliothèque native de l'hôte pour le bureau : JNA la cherche dans les ressources sous
// <os>-<arch>/ (ex. darwin-aarch64/libvetsim_ffi.dylib).
val osName = System.getProperty("os.name").lowercase()
val osArch = System.getProperty("os.arch").let { if (it == "amd64") "x86-64" else if (it == "arm64") "aarch64" else it }
val jnaPrefix: String = when {
    osName.contains("mac") -> "darwin-$osArch"
    osName.contains("win") -> "win32-$osArch"
    else -> "linux-$osArch"
}
val libFile: String = when {
    osName.contains("mac") -> "libvetsim_ffi.dylib"
    osName.contains("win") -> "vetsim_ffi.dll"
    else -> "libvetsim_ffi.so"
}

val cargoBuildHost by tasks.registering(Exec::class) {
    description = "Compile vetsim-ffi pour l'hôte (bureau JVM)."
    workingDir = kitDir
    commandLine(cargo, "build", "--release", "-p", "vetsim-ffi")
    inputs.dir(kitDir.resolve("crates"))
    outputs.file(kitDir.resolve("target/release/$libFile"))
}

val copyHostLib by tasks.registering(Copy::class) {
    dependsOn(cargoBuildHost)
    from(kitDir.resolve("target/release/$libFile"))
    into(genDir.map { it.dir("desktopResources/$jnaPrefix") })
}

val cargoBuildAndroid by tasks.registering(Exec::class) {
    description = "Compile vetsim-ffi pour Android (arm64-v8a, x86_64) avec cargo-ndk."
    workingDir = kitDir
    val sdk = System.getenv("ANDROID_HOME") ?: run {
        val lp = rootDir.resolve("local.properties")
        if (lp.exists()) lp.readLines().firstOrNull { it.startsWith("sdk.dir=") }?.substringAfter("=") else null
    } ?: "${System.getProperty("user.home")}/Library/Android/sdk"
    val ndk = System.getenv("ANDROID_NDK_HOME") ?: File(sdk, "ndk").listFiles()?.maxOfOrNull { it.path } ?: ""
    environment("ANDROID_NDK_HOME", ndk)
    commandLine(
        cargo, "ndk", "-t", "arm64-v8a", "-t", "x86_64", "-P", "24",
        "-o", genDir.get().dir("jniLibs").asFile.path, "build", "--release", "-p", "vetsim-ffi",
    )
    inputs.dir(kitDir.resolve("crates"))
    outputs.dir(genDir.map { it.dir("jniLibs") })
}

val uniffiBindings by tasks.registering(Exec::class) {
    description = "Génère les liaisons Kotlin (UniFFI) à partir de la bibliothèque compilée."
    dependsOn(cargoBuildHost)
    workingDir = kitDir
    val out = genDir.get().dir("uniffi").asFile
    doFirst { out.deleteRecursively(); out.mkdirs() }
    commandLine(
        cargo, "run", "-q", "--release", "-p", "uniffi-bindgen", "--", "generate",
        "--library", "target/release/$libFile", "--language", "kotlin", "--no-format", "--out-dir", out.path,
    )
    inputs.file(kitDir.resolve("target/release/$libFile"))
    outputs.dir(out)
}

val modulesBundle by tasks.registering {
    description = "Embarque modules/*.json dans une source Kotlin (format de loadModules)."
    val modules = kitDir.resolve("modules")
    val out = genDir.map { it.file("modules/fr/vetsim/demo/ModulesBundle.kt") }
    inputs.dir(modules)
    outputs.file(out)
    doLast {
        fun list(dir: String) = modules.resolve(dir).listFiles { f -> f.extension == "json" }!!
            .sortedBy { it.name }.joinToString(",\n") { it.readText().trim() }
        val bundle = "{\"patient\": ${modules.resolve("healthy_dog.json").readText().trim()},\n" +
            "\"diseases\": [\n${list("diseases")}],\n\"drugs\": [\n${list("drugs")}]}"
        // Chaîne brute Kotlin : seul « $ » doit être protégé.
        val escaped = bundle.replace("$", "\${'$'}")
        val f = out.get().asFile
        f.parentFile.mkdirs()
        f.writeText(
            "// Généré par la tâche modulesBundle à partir de ../modules : ne pas modifier.\n" +
                "package fr.vetsim.demo\n\n" +
                "/** Modules JSON du kit, au format attendu par `Engine.loadModules`. */\n" +
                "internal val MODULES_BUNDLE: String = \"\"\"$escaped\"\"\"\n",
        )
    }
}

kotlin {
    androidTarget {
        compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
    }
    jvm("desktop") {
        compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
    }

    // Source set commun aux deux cibles JVM : liaisons UniFFI (JNA).
    applyDefaultHierarchyTemplate {
        common {
            group("jvmShared") {
                withAndroidTarget()
                withJvm()
            }
        }
    }

    sourceSets {
        commonMain {
            kotlin.srcDir(modulesBundle.map { genDir.get().dir("modules") })
            dependencies {
                implementation(compose.runtime)
                implementation(compose.foundation)
                implementation(compose.material3)
                implementation(compose.ui)
                implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
            }
        }
        val jvmSharedMain by getting {
            kotlin.srcDir(uniffiBindings.map { genDir.get().dir("uniffi") })
            dependencies {
                compileOnly("net.java.dev.jna:jna:5.15.0")
            }
        }
        androidMain.dependencies {
            implementation("net.java.dev.jna:jna:5.15.0@aar")
            implementation("androidx.activity:activity-compose:1.10.0")
        }
        val desktopMain by getting {
            resources.srcDir(copyHostLib.map { genDir.get().dir("desktopResources") })
            dependencies {
                implementation(compose.desktop.currentOs)
                implementation("net.java.dev.jna:jna:5.15.0")
            }
        }
        val desktopTest by getting {
            dependencies {
                implementation(kotlin("test"))
                implementation("org.jetbrains.compose.ui:ui-test-junit4:1.7.3")
            }
        }
    }
}

android {
    namespace = "fr.vetsim.demo"
    compileSdk = 35
    defaultConfig {
        applicationId = "fr.vettwins.app"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }
    sourceSets["main"].jniLibs.srcDir(genDir.map { it.dir("jniLibs") })
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    packaging { resources.excludes += "/META-INF/{AL2.0,LGPL2.1}" }
}

tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach {
    dependsOn(cargoBuildAndroid)
}

compose.desktop {
    application {
        mainClass = "fr.vetsim.demo.MainKt"
        nativeDistributions {
            packageName = "VetTwins"
            packageVersion = "1.0.0"
        }
    }
}

val cargoBuildCli by tasks.registering(Exec::class) {
    description = "Compile vetsim-cli (référence native du test d'identité au bit près)."
    workingDir = kitDir
    commandLine(cargo, "build", "--release", "-p", "vetsim-cli")
}

tasks.withType<Test>().configureEach {
    dependsOn(cargoBuildCli)
    // Dossier de sortie des captures d'écran du test de rendu.
    systemProperty("vetsim.screenshots", layout.buildDirectory.dir("screenshots").get().asFile.path)
    systemProperty("vetsim.kit", kitDir.path)
}
