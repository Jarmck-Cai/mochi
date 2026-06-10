#include "brain_client.h"

#include <cstdio>

namespace mochi {

namespace {
const wchar_t kPipeName[] = L"\\\\.\\pipe\\mochi-brain-v0";

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
  return static_cast<ULONGLONG>(c.QuadPart) * 1000000ULL / freq;
}
}  // namespace

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
    ArmBackoff();
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
  ULONGLONG now = NowUs();
  // Ceil to whole ms so a sub-ms remainder never truncates to a 0ms wait.
  DWORD wait_ms = (now < deadline_us)
                      ? static_cast<DWORD>((deadline_us - now + 999) / 1000)
                      : 0;
  if (WaitForSingleObject(event_, wait_ms) == WAIT_OBJECT_0) {
    return GetOverlappedResult(pipe_, ov, transferred, FALSE) != 0;
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
