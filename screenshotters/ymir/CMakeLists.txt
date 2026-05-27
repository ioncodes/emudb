cmake_minimum_required(VERSION 3.28)

project(ymir-screenshotter
    VERSION 0.2.0
    LANGUAGES CXX
)

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON CACHE BOOL "" FORCE)

set(Ymir_DEV_BUILD            OFF CACHE BOOL "" FORCE)
set(Ymir_ENABLE_TESTS         OFF CACHE BOOL "" FORCE)
set(Ymir_ENABLE_SANDBOX       OFF CACHE BOOL "" FORCE)
set(Ymir_ENABLE_YMDASM        OFF CACHE BOOL "" FORCE)
set(Ymir_ENABLE_YMIR_HEADLESS OFF CACHE BOOL "" FORCE)
set(Ymir_ENABLE_YMIR_DBG      OFF CACHE BOOL "" FORCE)
set(Ymir_ENABLE_IPO           ON  CACHE BOOL "" FORCE)
set(Ymir_FEATUREFLAG_DEFAULT  OFF CACHE BOOL "" FORCE)

set(YMIR_CORE_DIR "/src/ymir" CACHE PATH "Path to the Ymir emulator source tree")
add_subdirectory(${YMIR_CORE_DIR} ymir-core-build EXCLUDE_FROM_ALL)

add_library(yscreen-driver STATIC
    src/worker_driver.cpp
    src/input_sequence.cpp
    src/png_writer.cpp
    src/game_id.cpp
    src/ipl_picker.cpp
    src/file_io.cpp
)
target_include_directories(yscreen-driver PUBLIC src)
target_link_libraries(yscreen-driver PUBLIC ymir::ymir-core)

add_executable(ymir-screenshotter-worker
    src/worker.cpp
)
target_link_libraries(ymir-screenshotter-worker PRIVATE yscreen-driver)
