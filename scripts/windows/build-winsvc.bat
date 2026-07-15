@echo off
setlocal
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul
set PATH=%USERPROFILE%\.cargo\bin;%PATH%
cd /d C:\ramshared\src
echo == cargo build -p ramshared-winsvc --release ==
cargo build -p ramshared-winsvc --release
if errorlevel 1 exit /b 1
if not exist target\release\ramshared-winsvc.exe (
  echo missing exe
  exit /b 2
)
mkdir C:\ramshared\bin 2>nul
mkdir C:\ProgramData\RamShared 2>nul
mkdir C:\ProgramData\RamShared\evidence 2>nul
copy /Y target\release\ramshared-winsvc.exe C:\ramshared\bin\ramshared-winsvc.exe
copy /Y crates\ramshared-winsvc\winsvc.example.toml C:\ProgramData\RamShared\winsvc.toml
echo BUILD_OK
dir C:\ramshared\bin\ramshared-winsvc.exe
exit /b 0
