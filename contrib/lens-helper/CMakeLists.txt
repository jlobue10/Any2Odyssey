# SPDX-License-Identifier: MIT
#
# Builds any2odyssey-lens-helper against Leia's srSDK. The SDK is commercial
# and is not in this repository: it comes with Leia's `leiasr-runtime` package
# (under /opt/leiasr), or point SRSDK_ROOT at an unpacked copy.
#
#   cmake -S contrib/lens-helper -B build/lens-helper
#   cmake --build build/lens-helper
#   sudo cmake --install build/lens-helper      # /usr/local/lib/any2odyssey/
cmake_minimum_required(VERSION 3.20)
project(any2odyssey-lens-helper LANGUAGES C)

if(DEFINED ENV{SRSDK_ROOT})
    list(APPEND CMAKE_PREFIX_PATH "$ENV{SRSDK_ROOT}")
endif()
list(APPEND CMAKE_PREFIX_PATH /opt/leiasr)

# Exports srSDK::loader, a static loader that opens libLeiaSR_runtime.so when
# first called; the runtime itself is found through the registration the
# runtime's own package writes.
find_package(srSDK 1.0 CONFIG REQUIRED)

add_executable(any2odyssey-lens-helper any2odyssey-lens-helper.c)
set_target_properties(any2odyssey-lens-helper PROPERTIES C_STANDARD 99 C_STANDARD_REQUIRED ON)
target_compile_options(any2odyssey-lens-helper PRIVATE -Wall -Wextra)
target_link_libraries(any2odyssey-lens-helper PRIVATE srSDK::loader)

install(TARGETS any2odyssey-lens-helper DESTINATION lib/any2odyssey)
