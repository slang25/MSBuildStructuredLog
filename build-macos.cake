#addin "Cake.Plist"

var target = Argument("target", "Default");
var platform = Argument("platform", "AnyCPU");
var configuration = Argument("configuration", "Release");

var version = EnvironmentVariable("APPVEYOR_BUILD_VERSION") ?? Argument("version", "0.0.1");

var artifactsDir = (DirectoryPath)Directory("./artifacts");

var netAppsRoot= "./src";
var netApp = "StructuredLogViewer.Avalonia";
var macAppName = "Structured Log Viewer";

var buildDirs = 
    GetDirectories($"{netAppsRoot}/**/bin/**") + 
    GetDirectories($"{netAppsRoot}/**/obj/**") + 
    GetDirectories($"{netAppsRoot}/artifacts/*");

var netProject = new {
        Path = $"{netAppsRoot}/{netApp}",
        Name = netApp,
        Framework = XmlPeek($"{netAppsRoot}/{netApp}/{netApp}.csproj", "//*[local-name()='TargetFramework']/text()"),
        Runtimes = XmlPeek($"{netAppsRoot}/{netApp}/{netApp}.csproj", "//*[local-name()='RuntimeIdentifiers']/text()").Split(';')
    };


 Task("Clean")
 .Does(()=>{
     CleanDirectories(buildDirs);
 });

 Task("Restore-Net")
     .IsDependentOn("Clean")
     .Does(() =>
 {
    DotNetRestore(netProject.Path);
 });

 Task("Build-Net")
     .IsDependentOn("Restore-Net")
     .Does(() =>
 {
    Information("Building: {0}", netProject.Name);
    DotNetBuild(netProject.Path, new DotNetBuildSettings {
        Configuration = configuration
    });
 });

 Task("Publish-Net")
     .IsDependentOn("Restore-Net")
     .Does(() =>
 {
    foreach(var runtime in netProject.Runtimes)
    {
        if (!runtime.Contains("osx"))
            continue;

        var outputDir = artifactsDir.Combine(runtime);

        Information("Publishing: {0}, runtime: {1}", netProject.Name, runtime);
        DotNetPublish(netProject.Path, new DotNetPublishSettings {
            Framework = netProject.Framework,
            Configuration = configuration,
            Runtime = runtime,
            SelfContained = true,
            OutputDirectory = outputDir.FullPath
        });
    }
 });


 Task("Package-Mac")
     .IsDependentOn("Publish-Net")
     .Does(() =>
 {
    var runtimeIdentifiers = netProject.Runtimes.Where(r => r.StartsWith("osx"));
    foreach(var runtime in runtimeIdentifiers)
    {
        var workingDir = artifactsDir.Combine(runtime);
        var tempDir = artifactsDir.Combine("app");

        Information("Copying Info.plist");
        EnsureDirectoryExists(tempDir.Combine("Contents"));
        CopyFileToDirectory($"{netAppsRoot}/{netApp}/Info.plist", tempDir.Combine("Contents"));
        
        // Update versions in Info.plist
        var plistFile = tempDir.Combine("Contents").CombineWithFilePath("Info.plist");
        dynamic plist = DeserializePlist(plistFile);
        plist["CFBundleShortVersionString"] = version;
        plist["CFBundleVersion"] = version;
        SerializePlist(plistFile, plist);

        Information("Copying App Icons");
        EnsureDirectoryExists(tempDir.Combine("Contents/Resources"));
        CopyFileToDirectory($"{netAppsRoot}/{netApp}/StructuredLogViewer.icns", tempDir.Combine("Contents/Resources"));
        
        Information("Copying executables");
        MoveDirectory(workingDir, tempDir.Combine("Contents/MacOS"));

        Information("Finish packaging");
        EnsureDirectoryExists(workingDir);
        MoveDirectory(tempDir, workingDir.Combine($"{macAppName}.app"));

        Zip(workingDir.FullPath, workingDir.CombineWithFilePath($"../{macAppName}-{string.SubString(runtime, "osx".Length).TrimStart('-')}.zip"));
    }
 });

 Task("Default")
     .IsDependentOn("Restore-Net")
     .IsDependentOn("Publish-Net")
     .IsDependentOn("Package-Mac");

 RunTarget(target);
