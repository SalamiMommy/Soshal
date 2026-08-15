allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

val newBuildDir: Directory =
    rootProject.layout.buildDirectory
        .dir("../../build")
        .get()
rootProject.layout.buildDirectory.value(newBuildDir)

subprojects {
    val newSubprojectBuildDir: Directory = newBuildDir.dir(project.name)
    project.layout.buildDirectory.value(newSubprojectBuildDir)
}
subprojects {
    project.evaluationDependsOn(":app")
}
subprojects {
    // camera-core 1.6.x declares androidx.concurrent:concurrent-futures with
    // `runtime` scope; Gradle 9 no longer promotes runtime dependencies onto the
    // compile classpath, so javac fails to attach the jspecify @NonNull type
    // annotations on SurfaceRequest ("class file for
    // androidx.concurrent.futures.CallbackToFutureAdapter not found"). Pin it
    // explicitly on the camerax module — it was already pulled in transitively
    // at runtime.
    if (name == "camera_android_camerax") {
        afterEvaluate {
            dependencies {
                add("implementation", "androidx.concurrent:concurrent-futures:1.2.0")
            }
        }
    }
}

tasks.register<Delete>("clean") {
    delete(rootProject.layout.buildDirectory)
}
