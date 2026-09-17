# Explicit developer setup only. The application never downloads executables.
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$toolsRoot = Join-Path $repo '.tools'
$downloads = Join-Path $toolsRoot 'pdf-standards'
$destination = Join-Path $toolsRoot 'verapdf'
if (Test-Path (Join-Path $destination 'bin/cli-1.30.2.jar')) {
    Write-Output 'veraPDF is already installed. See docs/PDF_A_EXPORT.md for runtime configuration.'
    exit 0
}
if (Test-Path $destination) { throw 'Refusing to replace an existing veraPDF folder.' }
New-Item -ItemType Directory -Force $downloads | Out-Null
function Download-Checked($Url, $File, $Sha256) {
    if (-not (Test-Path -LiteralPath $File)) { Invoke-WebRequest $Url -OutFile $File }
    if ((Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256.ToLowerInvariant()) { throw "Checksum mismatch: $File" }
}
Download-Checked 'https://software.verapdf.org/releases/1.30/verapdf-greenfield-1.30.2-installer.zip' (Join-Path $downloads 'verapdf.zip') '6cc6341cb1af644044054b81f00a6590a7918abb18f762243de115258bcad838'
Download-Checked 'https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12.1%2B1/OpenJDK21U-jre_x64_windows_hotspot_21.0.12.1_1.zip' (Join-Path $downloads 'jre.zip') 'd35f31e712f0fcf6ac5a093edc90204fbff22f720ba3950bd09d331d5e621636'
if (-not (Test-Path (Join-Path $downloads 'java'))) { Expand-Archive (Join-Path $downloads 'jre.zip') (Join-Path $downloads 'java') }
if (-not (Test-Path (Join-Path $downloads 'installer'))) { Expand-Archive (Join-Path $downloads 'verapdf.zip') (Join-Path $downloads 'installer') }
$javaExe = (Get-ChildItem (Join-Path $downloads 'java') -Recurse -Filter java.exe | Select-Object -First 1).FullName
$installerJar = (Get-ChildItem (Join-Path $downloads 'installer') -Recurse -Filter '*.jar' | Select-Object -First 1).FullName
if (-not $javaExe -or -not $installerJar) { throw 'Downloaded package layout is unexpected.' }
$escapedDestination = [Security.SecurityElement]::Escape($destination)
$configuration = @"
<AutomatedInstallation langpack="eng">
<com.izforge.izpack.panels.htmlhello.HTMLHelloPanel id="welcome"/>
<com.izforge.izpack.panels.target.TargetPanel id="install_dir"><installpath>$escapedDestination</installpath></com.izforge.izpack.panels.target.TargetPanel>
<com.izforge.izpack.panels.packs.PacksPanel id="sdk_pack_select">
<pack index="0" name="veraPDF GUI" selected="true"/>
<pack index="1" name="veraPDF CLI" selected="true"/>
<pack index="2" name="veraPDF Validation model" selected="false"/>
<pack index="3" name="veraPDF Documentation" selected="true"/>
<pack index="4" name="veraPDF Sample Plugins" selected="false"/>
</com.izforge.izpack.panels.packs.PacksPanel>
<com.izforge.izpack.panels.install.InstallPanel id="install"/>
<com.izforge.izpack.panels.finish.FinishPanel id="finish"/>
</AutomatedInstallation>
"@
$configurationPath = Join-Path $downloads 'install.xml'
[IO.File]::WriteAllText($configurationPath, $configuration)
& $javaExe -jar $installerJar $configurationPath
if ($LASTEXITCODE -ne 0) { throw 'veraPDF installation failed.' }
& $javaExe -cp (Join-Path $destination 'bin/*') org.verapdf.apps.GreenfieldCliWrapper --version
if ($LASTEXITCODE -ne 0) { throw 'veraPDF startup verification failed.' }
