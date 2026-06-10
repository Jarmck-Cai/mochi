# Mochi PoC translator —— librime 标准插件布局（merged-plugin 构建）。
# 用法：把本目录拷到 librime/plugins/mochi/，librime 根 CMake 会自动
# glob 收编（plugins/CMakeLists.txt 的 file(GLOB ...) 分支），并把
# plugin_modules "mochi" 注入 RIME_EXTRA_MODULES 实现自动加载。
# 参照 librime/sample/CMakeLists.txt 的插件分支（else 分支）。
project(rime-mochi)
cmake_minimum_required(VERSION 3.10)

aux_source_directory(src mochi_src)

set(mochi_library rime-mochi)
set(mochi_deps ${rime_library})
set(mochi_modules "mochi")

add_library(rime-mochi-objs OBJECT ${mochi_src})
if(BUILD_SHARED_LIBS)
  set_target_properties(rime-mochi-objs
    PROPERTIES
    POSITION_INDEPENDENT_CODE ON)
endif()

set(plugin_name ${mochi_library} PARENT_SCOPE)
set(plugin_objs $<TARGET_OBJECTS:rime-mochi-objs> PARENT_SCOPE)
set(plugin_deps ${mochi_deps} PARENT_SCOPE)
set(plugin_modules ${mochi_modules} PARENT_SCOPE)
