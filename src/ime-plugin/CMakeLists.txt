# Mochi ime-plugin — librime merged-plugin layout.
# Build flow: experiments/003-translator-poc/build-steps.cmd copies this
# directory to <librime>/plugins/mochi; librime's root CMake auto-globs it
# (plugins/CMakeLists.txt) and injects plugin module "mochi" into
# RIME_EXTRA_MODULES for automatic loading.
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
