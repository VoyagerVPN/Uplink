# Testing installation of a single Windows adapter

The patch targets the ZeroTier One 1.16.2 Windows service at commit `fc5c3ec22090b5b2a0f274e863651fe9ca489bf4`. During adapter creation, the service stages the signed INF in the Driver Store with `SetupCopyOEMInfA` and passes the specific `SP_DEVINFO_DATA` to `DiInstallDevice`. Windows selects a compatible driver from the store. On failure, the service requests removal of the device it just registered.

## Build the x64 service

Use Git, Rust with the MSVC toolchain, Visual Studio 2022 with v143, the Windows SDK, and x64 ATL and MFC. Run the commands from the Uplink root in Developer PowerShell. Reuse the prepared source checkout for subsequent builds.

```powershell
$root = (Get-Location).Path
$source = Join-Path $root '.cache/zerotier-129/source'
$env:CARGO_HOME = Join-Path $root '.cache/cargo'
$env:CARGO_TARGET_DIR = Join-Path $source 'rustybits/target'
git init $source
git -C $source fetch --depth 1 https://github.com/zerotier/ZeroTierOne.git refs/tags/1.16.2
if ($LASTEXITCODE) { throw 'Source fetch failed' }
git -C $source switch --detach FETCH_HEAD
if ($LASTEXITCODE -or (git -C $source rev-parse HEAD) -ne 'fc5c3ec22090b5b2a0f274e863651fe9ca489bf4') { throw 'Unexpected ZeroTier revision' }
git -C $source apply "$root/transport/windows/zerotier-1.16.2-device-install.patch"
if ($LASTEXITCODE) { throw 'Patch failed' }
cargo build --locked --manifest-path "$source/rustybits/Cargo.toml" -p zeroidc --release --target x86_64-pc-windows-msvc
if ($LASTEXITCODE) { throw 'zeroidc build failed' }
MSBuild "$source/windows/ZeroTierOne/ZeroTierOne.vcxproj" /p:Platform=x64 /p:Configuration=Release "/p:SolutionDir=$source/windows/" /p:BuildProjectReferences=false
if ($LASTEXITCODE) { throw 'Service build failed' }
```

The binary is at `windows/Build/x64/Release/zerotier-one_x64.exe` in that checkout. On the Windows client for test instance 129, only the service executable was copied to the protected installation directory. The signed SYS, CAT and INF came from the official MSI. To restore the original service, stop `ZeroTierOneService`, restore its saved `ImagePath` with `sc.exe config`, then start the service again.
