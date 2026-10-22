// Windows declarations precede replacement names, avoiding dllimport definitions.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <exception>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>
namespace clock_fixture {
extern LONGLONG frequency, counter;
extern unsigned opens, signals, cancellations, reaps, nonblocking_completions;
extern std::vector<DWORD> waits;
extern std::vector<LONGLONG> advances;
BOOL WINAPI Frequency(LARGE_INTEGER*);
BOOL WINAPI Counter(LARGE_INTEGER*);
HANDLE WINAPI Open(LPCWSTR,DWORD,DWORD,LPSECURITY_ATTRIBUTES,DWORD,DWORD,HANDLE);
BOOL WINAPI Launch(LPCWSTR,LPWSTR,LPSECURITY_ATTRIBUTES,LPSECURITY_ATTRIBUTES,BOOL,DWORD,LPVOID,LPCWSTR,LPSTARTUPINFOW,LPPROCESS_INFORMATION);
DWORD WINAPI Environment(LPCWSTR,LPWSTR,DWORD);
HANDLE WINAPI Event(LPSECURITY_ATTRIBUTES,BOOL,BOOL,LPCWSTR);
HANDLE WINAPI Thread(LPSECURITY_ATTRIBUTES,SIZE_T,LPTHREAD_START_ROUTINE,LPVOID,DWORD,LPDWORD);
BOOL WINAPI Close(HANDLE);
DWORD WINAPI Wait(HANDLE,DWORD);
BOOL WINAPI Signal(HANDLE);
BOOL WINAPI Cancel(HANDLE,LPOVERLAPPED);
BOOL WINAPI Complete(HANDLE,LPOVERLAPPED,LPDWORD,BOOL);
BOOL WINAPI State(HANDLE,LPDWORD,LPDWORD,LPDWORD);
BOOL WINAPI Write(HANDLE,LPCVOID,DWORD,LPDWORD,LPOVERLAPPED);
BOOL WINAPI Read(HANDLE,LPVOID,DWORD,LPDWORD,LPOVERLAPPED);
void Check(const char*,ULONGLONG,ULONGLONG);
int Finish();
int Error(const char*);
void Reset(LONGLONG);
}
#define QueryPerformanceFrequency clock_fixture::Frequency
#define QueryPerformanceCounter clock_fixture::Counter
#define GetTickCount64 TEAM_COARSE_CLOCK_IS_FORBIDDEN
#define GetTickCount TEAM_COARSE_CLOCK_IS_FORBIDDEN
#define CreateFileW clock_fixture::Open
#define CreateProcessW clock_fixture::Launch
#define GetEnvironmentVariableW clock_fixture::Environment
#define CreateEventW clock_fixture::Event
#define CreateThread clock_fixture::Thread
#define CloseHandle clock_fixture::Close
#define WaitForSingleObject clock_fixture::Wait
#define SetEvent clock_fixture::Signal
#define CancelIoEx clock_fixture::Cancel
#define GetOverlappedResult clock_fixture::Complete
#define SetNamedPipeHandleState clock_fixture::State
#define WriteFile clock_fixture::Write
#define ReadFile clock_fixture::Read
#define CreateFileA TEAM_ANSI_FILE_IS_FORBIDDEN
#define CreateProcessA TEAM_ANSI_PROCESS_IS_FORBIDDEN
#define GetEnvironmentVariableA TEAM_ANSI_ENVIRONMENT_IS_FORBIDDEN
#undef CreateFile
#undef CreateProcess
#undef GetEnvironmentVariable
#define CreateFile TEAM_GENERIC_FILE_IS_FORBIDDEN
#define CreateProcess TEAM_GENERIC_PROCESS_IS_FORBIDDEN
#define GetEnvironmentVariable TEAM_GENERIC_ENVIRONMENT_IS_FORBIDDEN
// Access only the existing client for controlled behavior checks; no product edit.
#define private public
#include "../src/brain_client.h"
#undef private

// This TU provides only synthetic operations. No clock, pipe or process is opened.
namespace clock_fixture {
LONGLONG frequency = 10000000, counter = 0;
unsigned opens = 0, signals = 0, cancellations = 0, reaps = 0, nonblocking_completions = 0;
std::vector<DWORD> waits;
std::vector<LONGLONG> advances;
unsigned assertions = 0, failures = 0;
static HANDLE event_handle = reinterpret_cast<HANDLE>(static_cast<uintptr_t>(0x101));
static HANDLE pipe_handle = reinterpret_cast<HANDLE>(static_cast<uintptr_t>(0x102));
static void Require(bool value, const char* error) { if (!value) throw std::runtime_error(error); }
void Reset(LONGLONG freq) {
  Require(freq > 0, "invalid fixture frequency"); frequency = freq; counter = 0;
  opens = signals = cancellations = reaps = nonblocking_completions = 0;
  assertions = failures = 0; waits.clear(); advances.clear();
}
BOOL WINAPI Frequency(LARGE_INTEGER* value) { Require(value != nullptr,"null frequency pointer"); value->QuadPart = frequency; return TRUE; }
BOOL WINAPI Counter(LARGE_INTEGER* value) { Require(value && counter >= 0,"invalid counter input"); value->QuadPart = counter; return TRUE; }
HANDLE WINAPI Open(LPCWSTR name,DWORD access,DWORD share,LPSECURITY_ATTRIBUTES security,DWORD disposition,DWORD flags,HANDLE file) {
  Require(name && std::wstring(name) == L"\\\\.\\pipe\\mochi-brain-v0" && access == (GENERIC_READ|GENERIC_WRITE) && !share && !security && disposition == OPEN_EXISTING && flags == FILE_FLAG_OVERLAPPED && !file,"unexpected synthetic open arguments");
  ++opens; SetLastError(ERROR_PIPE_BUSY); return INVALID_HANDLE_VALUE;
}
BOOL WINAPI Launch(LPCWSTR,LPWSTR,LPSECURITY_ATTRIBUTES,LPSECURITY_ATTRIBUTES,BOOL,DWORD,LPVOID,LPCWSTR,LPSTARTUPINFOW,LPPROCESS_INFORMATION) { throw std::runtime_error("unexpected process launch"); }
DWORD WINAPI Environment(LPCWSTR,LPWSTR,DWORD) { throw std::runtime_error("unexpected environment access"); }
HANDLE WINAPI Thread(LPSECURITY_ATTRIBUTES,SIZE_T,LPTHREAD_START_ROUTINE,LPVOID,DWORD,LPDWORD) { throw std::runtime_error("unexpected thread creation"); }
HANDLE WINAPI Event(LPSECURITY_ATTRIBUTES security,BOOL manual,BOOL initial,LPCWSTR name) { Require(!security && manual && !initial && !name,"unexpected event arguments"); return event_handle; }
BOOL WINAPI Close(HANDLE handle) { Require(handle==event_handle || handle==pipe_handle,"unexpected handle close"); return TRUE; }
BOOL WINAPI Signal(HANDLE handle) { Require(handle==event_handle,"unexpected signal handle"); ++signals; return TRUE; }
DWORD WINAPI Wait(HANDLE handle,DWORD milliseconds) {
  Require(handle==event_handle,"unexpected wait handle"); waits.push_back(milliseconds);
  // A bad implementation ends by completion, preserving an observable mismatch.
  // It never hangs until a tool timeout and never calls a real Windows wait.
  if (milliseconds > 15) return WAIT_OBJECT_0;
  Require(waits.size() <= advances.size(),"fixture wait sequence exhausted");
  counter = advances[waits.size()-1]; return WAIT_TIMEOUT;
}
BOOL WINAPI Cancel(HANDLE handle,LPOVERLAPPED op) { Require(handle==pipe_handle && op,"unexpected cancel"); ++cancellations; return TRUE; }
BOOL WINAPI Complete(HANDLE handle,LPOVERLAPPED op,LPDWORD transferred,BOOL blocking) {
  Require(handle==pipe_handle && op && transferred,"unexpected completion");
  if (blocking) ++reaps; else ++nonblocking_completions;
  *transferred = 0; return FALSE;
}
BOOL WINAPI State(HANDLE,LPDWORD,LPDWORD,LPDWORD) { throw std::runtime_error("unexpected pipe state I/O"); }
BOOL WINAPI Write(HANDLE,LPCVOID,DWORD,LPDWORD,LPOVERLAPPED) { throw std::runtime_error("unexpected write I/O"); }
BOOL WINAPI Read(HANDLE,LPVOID,DWORD,LPDWORD,LPOVERLAPPED) { throw std::runtime_error("unexpected read I/O"); }
void Check(const char* id,ULONGLONG actual,ULONGLONG expected) {
  ++assertions; const bool passed=actual==expected; if (!passed) ++failures;
  std::cout << "{\"schema\":\"mochi-clock-v1\",\"kind\":\"assert\",\"id\":\"" << id
    << "\",\"actual\":" << actual << ",\"expected\":" << expected << ",\"passed\":" << (passed?"true":"false") << "}\n";
}
int Finish() {
  std::cout << "{\"schema\":\"mochi-clock-v1\",\"kind\":\"summary\",\"tests\":" << assertions
    << ",\"failures\":" << failures << ",\"errors\":0,\"skipped\":0}\n";
  return failures?1:0;
}
int Error(const char*) {
  std::cerr << "clock fixture or candidate execution error\n";
  std::cout << "{\"schema\":\"mochi-clock-v1\",\"kind\":\"summary\",\"tests\":" << assertions
    << ",\"failures\":" << failures << ",\"errors\":1,\"skipped\":0}\n";
  return 2;
}
}

// Frozen team-owned native regression. The verifier supplies synthetic WinAPI.
#include "../src/brain_client.cc"
int main() {
  try {
    clock_fixture::Reset(10000000);
    clock_fixture::counter = 18446744073710LL;
    clock_fixture::Check("multiply-boundary", mochi::NowUs(), 1844674407371ULL);
    clock_fixture::counter = 9223372036854775807LL;
    clock_fixture::Check("long-counter", mochi::NowUs(), 922337203685477580ULL);
    return clock_fixture::Finish();
  } catch (...) { return clock_fixture::Error("candidate execution error"); }
}
