@echo off
rem Weasel fork feasibility build (stage-1 ghost UI groundwork).
rem Reuses librime's boost source tree; lessons from experiments/003 applied
rem (NoDefaultCurrentDirectoryInExePath injection breaks bare-name exec).
setlocal
set NoDefaultCurrentDirectoryInExePath=
set VSCMD_START_DIR=E:\Projects\P029_ai-ime\experiments\weasel-fork-probe
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cd /d E:\Projects\P029_ai-ime\experiments\weasel-fork-probe
if not exist env.bat copy env.vs2022.bat env.bat
set BOOST_ROOT=E:\Projects\P029_ai-ime\experiments\003-translator-poc\librime\deps\boost-1.89.0
echo === weasel all ===
call .\build.bat
if errorlevel 1 goto fail
if not exist output\Win32\weasel.dll if not exist output\weasel.dll (
  echo BUILD-PROBE-FAILED: no weasel.dll produced
  exit /b 1
)
echo BUILD-PROBE-OK
exit /b 0
:fail
echo BUILD-PROBE-FAILED %errorlevel%
exit /b 1
