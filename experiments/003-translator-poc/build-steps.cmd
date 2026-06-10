@echo off
rem Build librime deps + librime with the Mochi merged plugin.
rem Usage: build-steps.cmd [deps|librime|all]
setlocal
set TARGET=%1
if "%TARGET%"=="" set TARGET=all

rem This session sets NoDefaultCurrentDirectoryInExePath=1, which makes cmd
rem refuse to resolve bare names like "build.bat" from the CWD. Clear it for
rem this process tree so librime's official scripts work as designed.
set NoDefaultCurrentDirectoryInExePath=

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if errorlevel 1 exit /b 1

rem Prefer VS-bundled CMake (3.31): standalone CMake 4.x drops support for
rem the old cmake_minimum_required() in some librime deps.
set PATH=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;%PATH%

cd /d "%~dp0librime"
echo CWD=%CD%
where cmake
cmake --version

if "%TARGET%"=="librime" goto :librime
call build.bat deps
if errorlevel 1 exit /b 2
if "%TARGET%"=="deps" exit /b 0

:librime
call build.bat librime
if errorlevel 1 exit /b 3
exit /b 0
