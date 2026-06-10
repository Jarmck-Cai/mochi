// brain_client.h — named pipe client for the Mochi brain service.
// Contract: docs/specs/ipc-v0.md.
//
// Hot-path guarantees (the whole point of this class):
// - one CreateFile per (re)connect, the handle is reused across keystrokes
// - every roundtrip (write request + read response) is bounded by a hard
//   deadline via OVERLAPPED I/O + WaitForSingleObject; on deadline the
//   pending I/O is cancelled and reaped before returning
// - any timeout / pipe error drops the connection, marks the brain
//   unavailable and arms a 2s backoff: until it expires every call returns
//   false immediately (no connect attempt, no request queuing)
#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <string>
#include <vector>

namespace mochi {

class BrainClient {
 public:
  static constexpr DWORD kBudgetMs = 15;        // ipc-v0 hard budget
  static constexpr ULONGLONG kBackoffMs = 2000; // ipc-v0 reconnect backoff
  static constexpr size_t kMaxMessage = 64 * 1024;

  BrainClient();
  ~BrainClient();

  BrainClient(const BrainClient&) = delete;
  BrainClient& operator=(const BrainClient&) = delete;

  // Send `request`, wait for the response within kBudgetMs total.
  // Returns true and fills *response on success; false on timeout, pipe
  // error, or while the backoff window is armed (instant in that case).
  bool Roundtrip(const std::string& request, std::string* response);

  // True if the last roundtrip (or connect) succeeded.
  bool available() const { return available_; }

 private:
  bool EnsureConnected();
  void Drop();
  void ArmBackoff();
  // Wait for an overlapped op to finish before `deadline_us` (QPC µs; see
  // NowUs in the .cc — GetTickCount64 granularity broke the 15ms budget).
  // On timeout cancels and reaps the op so buffers stay safe.
  bool WaitOp(OVERLAPPED* ov, ULONGLONG deadline_us, DWORD* transferred);

  HANDLE pipe_ = INVALID_HANDLE_VALUE;
  HANDLE event_ = nullptr;        // manual-reset, reused for every op
  std::vector<char> read_buf_;    // kMaxMessage, reused
  ULONGLONG retry_after_us_ = 0;  // QPC-µs backoff gate
  bool available_ = false;
};

}  // namespace mochi
