# Import only compiler-related settings; do not echo the process environment.
$ErrorActionPreference = 'Stop'
if (-not $env:VCINSTALLDIR) {
    $vswherePath = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswherePath)) { throw 'Install Visual Studio C++ Build Tools and the Windows SDK.' }
    $visualStudioPath = & $vswherePath -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $visualStudioPath) { throw 'No Visual Studio C++ toolchain found.' }
    $developerCommand = Join-Path $visualStudioPath 'Common7\Tools\VsDevCmd.bat'
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $env:ComSpec
    $startInfo.Arguments = '/d /s /c ""' + $developerCommand + '" -no_logo -arch=x64 -host_arch=x64 >nul && set"'
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.CreateNoWindow = $true
    $compilerProcess = [System.Diagnostics.Process]::Start($startInfo)
    $compilerEnvironment = $compilerProcess.StandardOutput.ReadToEnd() -split "`r?`n"
    $compilerProcess.WaitForExit()
    if ($compilerProcess.ExitCode -ne 0) { throw 'Could not initialize the Visual Studio compiler environment.' }
    $compilerKeys = @('PATH','INCLUDE','LIB','LIBPATH','VCINSTALLDIR','VCToolsInstallDir','VSINSTALLDIR','WindowsSdkDir','WindowsSDKVersion','WindowsSdkBinPath','WindowsSdkVerBinPath','UniversalCRTSdkDir','UCRTVersion','VSCMD_ARG_TGT_ARCH','VSCMD_ARG_HOST_ARCH','VSCMD_VER')
    foreach ($line in $compilerEnvironment) {
        $parts = $line -split '=', 2
        if ($parts.Count -eq 2 -and $parts[0] -in $compilerKeys) { [Environment]::SetEnvironmentVariable($parts[0],$parts[1],'Process') }
    }
}
if (-not $env:LIBCLANG_PATH) { $env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin' }
# Use an explicit target rather than relying on libclang's host defaults.
if (-not $env:BINDGEN_EXTRA_CLANG_ARGS) { $env:BINDGEN_EXTRA_CLANG_ARGS = '--target=x86_64-pc-windows-msvc' }
$projectSdk = Join-Path $PSScriptRoot '..\target\asio-sdk-download\ASIOSDK'
if (-not $env:CPAL_ASIO_DIR -and (Test-Path -LiteralPath (Join-Path $projectSdk 'common\asio.h'))) {
    $env:CPAL_ASIO_DIR = (Resolve-Path -LiteralPath $projectSdk).Path
}
