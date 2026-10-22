// Frozen synthetic, pure-memory public Parse regression. No host IPC or Brain.
#include "../src/mini_json.h"
#include <cmath>
#include <iomanip>
#include <iostream>
#include <limits>
#include <locale>
#include <string>
#include <utility>
#include <vector>
using mochi::json::Value;
using Type = Value::Type;
namespace {
Value Num(double n) { Value v; v.type=Type::kNumber; v.number=n; return v; }
Value Bool(bool b) { Value v; v.type=Type::kBool; v.boolean=b; return v; }
Value Str(const char* s) { Value v; v.type=Type::kString; v.str=s; return v; }
Value Arr(std::initializer_list<Value> values) { Value v; v.type=Type::kArray; v.array=values; return v; }
Value Obj(std::initializer_list<std::pair<const std::string,Value>> values) {
  Value v; v.type=Type::kObject; v.object=values; return v;
}
Value Response() { return Obj({{"v",Num(1)},{"candidates",Arr({})},{"elapsed_us",Num(0)}}); }
const char* Kind(const Value& v) {
  switch(v.type) {
    case Type::kNull:return "null"; case Type::kBool:return "bool";
    case Type::kNumber:return "number"; case Type::kString:return "string";
    case Type::kArray:return "array"; case Type::kObject:return "object";
  }
  return "invalid";
}
void Quote(const std::string& text) {
  static const char* hex="0123456789abcdef";
  std::cout << '"';
  for(unsigned char c:text) {
    switch(c) {
      case '"':std::cout << "\\\"";break; case '\\':std::cout << "\\\\";break;
      case '\n':std::cout << "\\n";break;case '\r':std::cout << "\\r";break;
      case '\t':std::cout << "\\t";break;
      default:
        if(c<32) std::cout << "\\u00" << hex[c>>4] << hex[c&15];
        else std::cout << static_cast<char>(c);
    }
  }
  std::cout << '"';
}
void Emit(const Value& v) {
  switch(v.type) {
    case Type::kNull:std::cout << "null";break;
    case Type::kBool:std::cout << (v.boolean?"true":"false");break;
    case Type::kNumber:
      if(std::isfinite(v.number)) std::cout << std::setprecision(std::numeric_limits<double>::max_digits10) << v.number;
      else Quote(std::isnan(v.number)?"nan":(v.number>0?"+infinity":"-infinity"));
      break;
    case Type::kString:Quote(v.str);break;
    case Type::kArray:
      std::cout << '[';
      for(size_t i=0;i<v.array.size();++i) { if(i)std::cout << ',';Emit(v.array[i]); }
      std::cout << ']';break;
    case Type::kObject: {
      std::cout << '{';bool first=true;
      for(const auto& item:v.object) { if(!first)std::cout << ',';first=false;Quote(item.first);std::cout << ':';Emit(item.second); }
      std::cout << '}';break;
    }
  }
}
bool Same(const Value& a,const Value& b) {
  if(a.type!=b.type) return false;
  switch(a.type) {
    case Type::kNull:return true;
    case Type::kBool:return a.boolean==b.boolean;
    case Type::kNumber:return a.number==b.number;
    case Type::kString:return a.str==b.str;
    case Type::kArray:
      if(a.array.size()!=b.array.size())return false;
      for(size_t i=0;i<a.array.size();++i)if(!Same(a.array[i],b.array[i]))return false;
      return true;
    case Type::kObject:
      if(a.object.size()!=b.object.size())return false;
      for(const auto& item:a.object) {
        auto it=b.object.find(item.first);
        if(it==b.object.end()||!Same(item.second,it->second))return false;
      }
      return true;
  }
  return false;
}
struct Test { const char* id; const char* input; bool accepted; Value expected; bool check; };
Test Yes(const char* id,const char* text,Value value) { return {id,text,true,std::move(value),true}; }
Test No(const char* id,const char* text) { return {id,text,false,Value{},false}; }
Test Range(const char* id,const char* text) { return {id,text,true,Num(0),false}; }
int Run(const std::vector<Test>& tests) {
  int failures=0;
  for(const Test& t:tests) {
    Value actual;
    const bool accepted=mochi::json::Parse(t.input,&actual);
    // Rejected Parse does not impose a rollback policy on partial output.
    const bool passed=accepted==t.accepted && (!accepted ||
        (actual.type==t.expected.type && (!t.check || Same(actual,t.expected))));
    if(!passed)++failures;
    std::cout << "{\"schema\":\"mochi-json-number-v1\",\"kind\":\"assert\",\"id\":";
    Quote(t.id);std::cout << ",\"input\":";Quote(t.input);
    std::cout << ",\"actual\":{\"accepted\":" << (accepted?"true":"false") << ",\"type\":";
    Quote(accepted?Kind(actual):"unobserved");std::cout << ",\"value\":";
    if(accepted)Emit(actual);else std::cout << "null";
    std::cout << "},\"expected\":{\"accepted\":" << (t.accepted?"true":"false") << ",\"type\":";
    Quote(t.accepted?Kind(t.expected):"unobserved");std::cout << ",\"value\":";
    if(t.check)Emit(t.expected);else std::cout << "null";
    std::cout << ",\"check_value\":" << (t.check?"true":"false") << "},\"passed\":" << (passed?"true":"false") << "}\n";
  }
  std::cout << "{\"schema\":\"mochi-json-number-v1\",\"kind\":\"summary\",\"tests\":" << tests.size()
            << ",\"failures\":" << failures << ",\"errors\":0,\"skipped\":0}\n";
  return failures?1:0;
}
}  // namespace

int main(int argc,char**) {
  std::cout.imbue(std::locale::classic());
  if(argc!=1)return 2;
  return Run({
    Yes("valid-integer","1",Num(1)),Yes("valid-float","-1.25e+2",Num(-125)),
    Yes("valid-response",R"({"v":1,"candidates":[],"elapsed_us":0})",Response()),
    No("bare-plus","+"),No("bare-minus","-"),No("incomplete-exponent","1e"),
    No("embedded-minus","1-2"),No("leading-zero","01"),No("trailing-dot","1."),
    No("bad-object-version",R"({"v":1e,"candidates":[],"elapsed_us":0})")});
}
