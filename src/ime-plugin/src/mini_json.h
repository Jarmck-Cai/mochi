// mini_json.h — minimal hand-written JSON parser + string escaper for the
// Mochi IPC client (docs/specs/ipc-v0.md).
//
// Why hand-written: librime bundles yaml-cpp but no JSON library, and the
// v0 protocol only needs to (a) escape one input string into a request and
// (b) parse one small response object {v, candidates[], elapsed_us}. ~200
// lines of header beat vendoring a third-party single-header library into
// the merged-plugin build. Supported: object, array, string (incl. \uXXXX
// escapes -> UTF-8, with surrogate pairs), number, true/false/null.
// Not supported (not needed): comments, NaN/Inf, duplicate-key semantics.
#pragma once

#include <cstdint>
#include <cstdlib>
#include <map>
#include <string>
#include <vector>

namespace mochi {
namespace json {

class Value {
 public:
  enum class Type { kNull, kBool, kNumber, kString, kArray, kObject };

  Type type = Type::kNull;
  bool boolean = false;
  double number = 0.0;
  std::string str;
  std::vector<Value> array;
  std::map<std::string, Value> object;

  const Value* Find(const std::string& key) const {
    if (type != Type::kObject)
      return nullptr;
    auto it = object.find(key);
    return it == object.end() ? nullptr : &it->second;
  }
  std::string GetString(const std::string& key,
                        const std::string& fallback = std::string()) const {
    const Value* v = Find(key);
    return (v && v->type == Type::kString) ? v->str : fallback;
  }
  double GetNumber(const std::string& key, double fallback = 0.0) const {
    const Value* v = Find(key);
    return (v && v->type == Type::kNumber) ? v->number : fallback;
  }
};

namespace detail {

class Parser {
 public:
  Parser(const char* begin, const char* end) : p_(begin), end_(end) {}

  bool Parse(Value* out) {
    SkipWs();
    if (!ParseValue(out))
      return false;
    SkipWs();
    return p_ == end_;  // no trailing garbage
  }

 private:
  void SkipWs() {
    while (p_ != end_ &&
           (*p_ == ' ' || *p_ == '\t' || *p_ == '\n' || *p_ == '\r'))
      ++p_;
  }
  bool Literal(const char* lit) {
    const char* q = p_;
    while (*lit) {
      if (q == end_ || *q != *lit)
        return false;
      ++q;
      ++lit;
    }
    p_ = q;
    return true;
  }
  bool ParseValue(Value* out) {
    if (p_ == end_)
      return false;
    switch (*p_) {
      case '{':
        return ParseObject(out);
      case '[':
        return ParseArray(out);
      case '"':
        out->type = Value::Type::kString;
        return ParseString(&out->str);
      case 't':
        out->type = Value::Type::kBool;
        out->boolean = true;
        return Literal("true");
      case 'f':
        out->type = Value::Type::kBool;
        out->boolean = false;
        return Literal("false");
      case 'n':
        out->type = Value::Type::kNull;
        return Literal("null");
      default:
        return ParseNumber(out);
    }
  }
  bool ParseObject(Value* out) {
    out->type = Value::Type::kObject;
    ++p_;  // '{'
    SkipWs();
    if (p_ != end_ && *p_ == '}') {
      ++p_;
      return true;
    }
    while (p_ != end_) {
      SkipWs();
      std::string key;
      if (p_ == end_ || *p_ != '"' || !ParseString(&key))
        return false;
      SkipWs();
      if (p_ == end_ || *p_ != ':')
        return false;
      ++p_;
      SkipWs();
      Value v;
      if (!ParseValue(&v))
        return false;
      out->object.emplace(std::move(key), std::move(v));
      SkipWs();
      if (p_ == end_)
        return false;
      if (*p_ == ',') {
        ++p_;
        continue;
      }
      if (*p_ == '}') {
        ++p_;
        return true;
      }
      return false;
    }
    return false;
  }
  bool ParseArray(Value* out) {
    out->type = Value::Type::kArray;
    ++p_;  // '['
    SkipWs();
    if (p_ != end_ && *p_ == ']') {
      ++p_;
      return true;
    }
    while (p_ != end_) {
      Value v;
      SkipWs();
      if (!ParseValue(&v))
        return false;
      out->array.emplace_back(std::move(v));
      SkipWs();
      if (p_ == end_)
        return false;
      if (*p_ == ',') {
        ++p_;
        continue;
      }
      if (*p_ == ']') {
        ++p_;
        return true;
      }
      return false;
    }
    return false;
  }
  bool ParseHex4(uint32_t* out) {
    uint32_t v = 0;
    for (int i = 0; i < 4; ++i) {
      if (p_ == end_)
        return false;
      char c = *p_++;
      v <<= 4;
      if (c >= '0' && c <= '9')
        v |= static_cast<uint32_t>(c - '0');
      else if (c >= 'a' && c <= 'f')
        v |= static_cast<uint32_t>(c - 'a' + 10);
      else if (c >= 'A' && c <= 'F')
        v |= static_cast<uint32_t>(c - 'A' + 10);
      else
        return false;
    }
    *out = v;
    return true;
  }
  static void AppendUtf8(uint32_t cp, std::string* s) {
    if (cp < 0x80) {
      s->push_back(static_cast<char>(cp));
    } else if (cp < 0x800) {
      s->push_back(static_cast<char>(0xC0 | (cp >> 6)));
      s->push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else if (cp < 0x10000) {
      s->push_back(static_cast<char>(0xE0 | (cp >> 12)));
      s->push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
      s->push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else {
      s->push_back(static_cast<char>(0xF0 | (cp >> 18)));
      s->push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
      s->push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
      s->push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    }
  }
  bool ParseString(std::string* out) {
    ++p_;  // opening quote
    while (p_ != end_) {
      unsigned char c = static_cast<unsigned char>(*p_);
      if (c == '"') {
        ++p_;
        return true;
      }
      if (c == '\\') {
        ++p_;
        if (p_ == end_)
          return false;
        char e = *p_++;
        switch (e) {
          case '"': out->push_back('"'); break;
          case '\\': out->push_back('\\'); break;
          case '/': out->push_back('/'); break;
          case 'b': out->push_back('\b'); break;
          case 'f': out->push_back('\f'); break;
          case 'n': out->push_back('\n'); break;
          case 'r': out->push_back('\r'); break;
          case 't': out->push_back('\t'); break;
          case 'u': {
            uint32_t cp = 0;
            if (!ParseHex4(&cp))
              return false;
            if (cp >= 0xD800 && cp <= 0xDBFF) {  // high surrogate
              uint32_t lo = 0;
              if (p_ == end_ || *p_ != '\\' || p_ + 1 == end_ ||
                  *(p_ + 1) != 'u')
                return false;
              p_ += 2;
              if (!ParseHex4(&lo) || lo < 0xDC00 || lo > 0xDFFF)
                return false;
              cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
            }
            AppendUtf8(cp, out);
            break;
          }
          default:
            return false;
        }
      } else {
        out->push_back(static_cast<char>(c));
        ++p_;
      }
    }
    return false;  // unterminated
  }
  bool ParseNumber(Value* out) {
    const char* start = p_;
    if (p_ != end_ && (*p_ == '-' || *p_ == '+'))
      ++p_;
    while (p_ != end_ && ((*p_ >= '0' && *p_ <= '9') || *p_ == '.' ||
                          *p_ == 'e' || *p_ == 'E' || *p_ == '-' || *p_ == '+'))
      ++p_;
    if (p_ == start)
      return false;
    out->type = Value::Type::kNumber;
    out->number = std::strtod(std::string(start, p_).c_str(), nullptr);
    return true;
  }

  const char* p_;
  const char* end_;
};

}  // namespace detail

inline bool Parse(const std::string& text, Value* out) {
  detail::Parser parser(text.data(), text.data() + text.size());
  return parser.Parse(out);
}

// Escape a UTF-8 string for embedding in a JSON string literal. Multi-byte
// UTF-8 passes through untouched (valid JSON); only quotes, backslashes and
// control characters are escaped.
inline std::string Escape(const std::string& s) {
  std::string out;
  out.reserve(s.size() + 8);
  static const char* kHex = "0123456789abcdef";
  for (unsigned char c : s) {
    switch (c) {
      case '"': out += "\\\""; break;
      case '\\': out += "\\\\"; break;
      case '\b': out += "\\b"; break;
      case '\f': out += "\\f"; break;
      case '\n': out += "\\n"; break;
      case '\r': out += "\\r"; break;
      case '\t': out += "\\t"; break;
      default:
        if (c < 0x20) {
          out += "\\u00";
          out += kHex[(c >> 4) & 0xF];
          out += kHex[c & 0xF];
        } else {
          out += static_cast<char>(c);
        }
    }
  }
  return out;
}

}  // namespace json
}  // namespace mochi
