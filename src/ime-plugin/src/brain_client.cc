#include "brain_client.h"

#include <cstdio>

namespace mochi {

namespace {
const wchar_t kPipeName[] = L"\\\\.\\pipe\\mochi-brain-v0";

// Compute floor(remainder * 1000000 / frequency), with remainder < frequency.
ULONGLONG FractionUs(ULONGLONG remainder, ULONGLONG frequency) {
  if (remainder <= ~0ULL / 1000000ULL)
    return remainder * 1000000ULL / frequency;

  // Binary long multiplication/division keeps each partial remainder below
  // frequency. Since frequency <= INT64_MAX, doubling or adding two reduced
  // remainders fits in unsigned 64 bits, even at extreme QPC frequencies.
  ULONGLONG quotient = 0;
  ULONGLONG partial = 0;
  for (unsigned bit = 1U << 19; bit; bit >>= 1) {
    quotient *= 2;
    partial *= 2;
    if (partial >= frequency) {
      partial -= frequency;
      ++quotient;
    }
    if (1000000U & bit) {
      partial += remainder;
      if (partial >= frequency) {
        partial -= frequency;
        ++quotient;
      }
    }
  }
  return quotient;
}

// GetTickCount64 has 10-16ms granularity — useless against a 15ms budget
// (quantization produced spurious instant timeouts in E2E testing). QPC is
// steady, ~µs-resolution, and cheap (~20ns per call on modern Windows).
ULONGLONG NowUs() {
  static const ULONGLONG freq = [] {
    LARGE_INTEGER f;
    QueryPerformanceFrequency(&f);
    return static_cast<ULONGLONG>(f.QuadPart);
  }();
  LARGE_INTEGER c;
  QueryPerformanceCounter(&c);
  const ULONGLONG counter = static_cast<ULONGLONG>(c.QuadPart);
  return (counter / freq) * 1000000ULL + FractionUs(counter % freq, freq);
}

// Lifecycle calls are serialized separately so joining never holds the state
// lock. Keys only try the state lock; they never wait for the worker.
SRWLOCK launcher_lifecycle = SRWLOCK_INIT;
SRWLOCK launcher_lock = SRWLOCK_INIT;
HANDLE launcher_event = nullptr;
HANDLE launcher_thread = nullptr;
bool launcher_accepting = false;
bool launcher_pending = false;  // Includes a launch currently in progress.
ULONGLONG launcher_retry_after_us = 0;

void LaunchBrain() {
  const DWORD size = GetEnvironmentVariableW(L"LOCALAPPDATA", nullptr, 0);
  if (!size)
    return;
  std::vector<wchar_t> buffer(size);
  const DWORD length = GetEnvironmentVariableW(L"LOCALAPPDATA", buffer.data(), size);
  if (!length || length >= size)
    return;
  std::wstring root(buffer.data(), length);
  // Require a fully qualified local path; never resolve a relative executable
  // against the host's working directory or search PATH.
  if (root.size() < 3 ||
      !((root[0] >= L'A' && root[0] <= L'Z') ||
        (root[0] >= L'a' && root[0] <= L'z')) ||
      root[1] != L':' || (root[2] != L'\\' && root[2] != L'/') ||
      root.find(L'"') != std::wstring::npos)
    return;
  while (root.size() > 3 && (root.back() == L'\\' || root.back() == L'/'))
    root.pop_back();
  root += L"\\Mochi";
  const std::wstring application = root + L"\\bin\\mochi-brain.exe";
  const std::wstring command = L"\"" + application + L"\" --artifacts \"" +
      root + L"\\artifacts\\v0\" --user-data \"" + root + L"\\user_data\"";
  std::vector<wchar_t> mutable_command(command.begin(), command.end());
  mutable_command.push_back(L'\0');
  STARTUPINFOW startup{};
  startup.cb = sizeof(startup);
  startup.dwFlags = STARTF_USESHOWWINDOW;
  startup.wShowWindow = SW_HIDE;
  PROCESS_INFORMATION process{};
  CreateProcessW(application.c_str(), mutable_command.data(), nullptr, nullptr,
                 FALSE, CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process);
  // No wait for Brain startup or exit, and no owned handles escape this call.
  if (process.hThread)
    CloseHandle(process.hThread);
  if (process.hProcess)
    CloseHandle(process.hProcess);
}

DWORD WINAPI BrainLauncherWorker(void*) {
  for (;;) {
    if (WaitForSingleObject(launcher_event, INFINITE) != WAIT_OBJECT_0)
      return 0;
    AcquireSRWLockExclusive(&launcher_lock);
    const bool stopping = !launcher_accepting;
    const bool pending = launcher_pending;
    ReleaseSRWLockExclusive(&launcher_lock);
    if (stopping)
      return 0;
    if (pending) {
      try {
        LaunchBrain();
      } catch (...) {
        // Allocation failure degrades exactly like a failed process launch.
      }
      AcquireSRWLockExclusive(&launcher_lock);
      launcher_pending = false;
      ReleaseSRWLockExclusive(&launcher_lock);
    }
  }
}

void RequestBrainLaunch() {
  if (!TryAcquireSRWLockExclusive(&launcher_lock))
    return;
  const ULONGLONG now = NowUs();
  if (launcher_accepting && !launcher_pending && now >= launcher_retry_after_us) {
    launcher_pending = true;
    launcher_retry_after_us = now + BrainClient::kBackoffMs * 1000ULL;
    SetEvent(launcher_event);
  }
  ReleaseSRWLockExclusive(&launcher_lock);
}
}  // namespace

void StartBrainLauncher() {
  AcquireSRWLockExclusive(&launcher_lifecycle);
  AcquireSRWLockExclusive(&launcher_lock);
  if (!launcher_thread) {
    launcher_event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (launcher_event) {
      launcher_pending = false;
      launcher_accepting = true;
      launcher_thread = CreateThread(nullptr, 0, BrainLauncherWorker, nullptr, 0, nullptr);
      if (!launcher_thread) {
        launcher_accepting = false;
        CloseHandle(launcher_event);
        launcher_event = nullptr;
      }
    }
  }
  ReleaseSRWLockExclusive(&launcher_lock);
  ReleaseSRWLockExclusive(&launcher_lifecycle);
}

void StopBrainLauncher() {
  AcquireSRWLockExclusive(&launcher_lifecycle);
  AcquireSRWLockExclusive(&launcher_lock);
  launcher_accepting = false;
  if (launcher_event)
    SetEvent(launcher_event);
  ReleaseSRWLockExclusive(&launcher_lock);
  if (launcher_thread) {
    WaitForSingleObject(launcher_thread, INFINITE);
    CloseHandle(launcher_thread);
    launcher_thread = nullptr;
  }
  AcquireSRWLockExclusive(&launcher_lock);
  if (launcher_event) {
    CloseHandle(launcher_event);
    launcher_event = nullptr;
  }
  launcher_pending = false;
  ReleaseSRWLockExclusive(&launcher_lock);
  ReleaseSRWLockExclusive(&launcher_lifecycle);
}

BrainClient::BrainClient() : read_buf_(kMaxMessage) {
  // Manual-reset so GetOverlappedResult/WaitForSingleObject semantics stay
  // simple; reset implicitly by each new overlapped op.
  event_ = CreateEventW(nullptr, /*bManualReset=*/TRUE,
                        /*bInitialState=*/FALSE, nullptr);
}

BrainClient::~BrainClient() {
  Drop();
  if (event_)
    CloseHandle(event_);
}

bool BrainClient::EnsureConnected() {
  if (pipe_ != INVALID_HANDLE_VALUE)
    return true;
  if (NowUs() < retry_after_us_)
    return false;  // backoff window armed: fail instantly, no syscall storm
  // CreateFile on a local pipe with a free server instance completes
  // immediately. If all instances are busy (ERROR_PIPE_BUSY) we deliberately
  // do NOT call WaitNamedPipe (it blocks); fail fast and back off.
  HANDLE h = CreateFileW(kPipeName, GENERIC_READ | GENERIC_WRITE,
                         /*dwShareMode=*/0, nullptr, OPEN_EXISTING,
                         FILE_FLAG_OVERLAPPED, nullptr);
  if (h == INVALID_HANDLE_VALUE) {
    const DWORD error = GetLastError();
    ArmBackoff();
    if (error == ERROR_FILE_NOT_FOUND)
      RequestBrainLaunch();
    return false;
  }
  DWORD mode = PIPE_READMODE_MESSAGE;
  if (!SetNamedPipeHandleState(h, &mode, nullptr, nullptr)) {
    CloseHandle(h);
    ArmBackoff();
    return false;
  }
  pipe_ = h;
  available_ = true;
  std::fprintf(stderr, "[mochi] brain connected\n");
  return true;
}

void BrainClient::Drop() {
  if (pipe_ != INVALID_HANDLE_VALUE) {
    CloseHandle(pipe_);
    pipe_ = INVALID_HANDLE_VALUE;
  }
  available_ = false;
}

void BrainClient::ArmBackoff() {
  retry_after_us_ = NowUs() + kBackoffMs * 1000ULL;
  available_ = false;
}

bool BrainClient::WaitOp(OVERLAPPED* ov,
                         ULONGLONG deadline_us,
                         DWORD* transferred) {
  // WaitForSingleObject timeouts are quantized to the system timer tick
  // (10-16ms): a 15ms wait that starts just before a tick boundary can
  // return WAIT_TIMEOUT after well under 1ms (observed ~0.5ms on ~4% of
  // keystrokes, each costing a spurious 2s degradation). The deadline is
  // therefore enforced with QPC only; an early WAIT_TIMEOUT re-waits for
  // the remainder instead of being trusted.
  for (;;) {
    ULONGLONG now = NowUs();
    if (now >= deadline_us)
      break;
    // Ceil to whole ms so a sub-ms remainder never truncates to a 0ms wait.
    DWORD wait_ms = static_cast<DWORD>((deadline_us - now + 999) / 1000);
    if (WaitForSingleObject(event_, wait_ms) == WAIT_OBJECT_0) {
      return GetOverlappedResult(pipe_, ov, transferred, FALSE) != 0;
    }
  }
  // Deadline blown: cancel, then *reap* the op so the kernel is done with
  // our buffers before the caller's stack/members are reused.
  CancelIoEx(pipe_, ov);
  GetOverlappedResult(pipe_, ov, transferred, TRUE);
  return false;
}

bool BrainClient::Roundtrip(const std::string& request, std::string* response) {
  if (!event_ || request.size() > kMaxMessage)
    return false;
  const ULONGLONG deadline_us = NowUs() + kBudgetMs * 1000ULL;
  if (!EnsureConnected())
    return false;

  // --- write request (one message) ---
  OVERLAPPED ov{};
  ov.hEvent = event_;
  DWORD transferred = 0;
  BOOL ok = WriteFile(pipe_, request.data(),
                      static_cast<DWORD>(request.size()), nullptr, &ov);
  if (!ok && GetLastError() != ERROR_IO_PENDING) {
    Drop();
    ArmBackoff();
    return false;
  }
  if (!WaitOp(&ov, deadline_us, &transferred) ||
      transferred != request.size()) {
    Drop();
    ArmBackoff();
    return false;
  }

  // --- read response (one message; PIPE_READMODE_MESSAGE frames it) ---
  OVERLAPPED ov2{};
  ov2.hEvent = event_;
  transferred = 0;
  ok = ReadFile(pipe_, read_buf_.data(), static_cast<DWORD>(read_buf_.size()),
                nullptr, &ov2);
  if (!ok && GetLastError() != ERROR_IO_PENDING) {
    // Includes ERROR_MORE_DATA (message over the 64KB protocol cap) and
    // ERROR_BROKEN_PIPE: treat all the same — degrade.
    Drop();
    ArmBackoff();
    return false;
  }
  if (!WaitOp(&ov2, deadline_us, &transferred) || transferred == 0) {
    Drop();
    ArmBackoff();
    return false;
  }
  response->assign(read_buf_.data(), transferred);
  available_ = true;
  return true;
}

}  // namespace mochi
