cmake_minimum_required(VERSION 3.16)

project(mai2io_affine C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)

add_library(affine_common STATIC
    common/src/dprintf.c
    common/src/serial_win.c
    common/src/affine_serial.c
)

target_include_directories(affine_common PUBLIC common/include)

target_compile_definitions(affine_common PUBLIC
    WIN32_LEAN_AND_MEAN
    _CRT_SECURE_NO_WARNINGS
)

add_library(mai2io SHARED
    mai2io/src/affine_io.c
    mai2io/src/mai2io.c
    mai2io/mai2io.def
)

target_include_directories(mai2io PRIVATE mai2io/include)
target_link_libraries(mai2io PRIVATE affine_common setupapi)

set_target_properties(mai2io PROPERTIES OUTPUT_NAME "mai2io")

add_library(chuniio_affine SHARED
    chuniio/src/chuniio.c
    chuniio/chuniio.def
)

target_include_directories(chuniio_affine PRIVATE chuniio/include)
target_link_libraries(chuniio_affine PRIVATE affine_common setupapi)
set_target_properties(chuniio_affine PROPERTIES OUTPUT_NAME "chuniio_affine")

add_library(mercuryio_affine SHARED
    mercuryio/src/mercuryio.c
    mercuryio/mercuryio.def
)

target_include_directories(mercuryio_affine PRIVATE mercuryio/include)
target_link_libraries(mercuryio_affine PRIVATE affine_common setupapi)
set_target_properties(mercuryio_affine PROPERTIES OUTPUT_NAME "mercuryio_affine")

add_library(aimeio_affine SHARED
    aimeio/src/aimeio.c
    aimeio/aimeio.def
)

target_include_directories(aimeio_affine PRIVATE aimeio/include)
target_link_libraries(aimeio_affine PRIVATE affine_common setupapi)
set_target_properties(aimeio_affine PROPERTIES OUTPUT_NAME "aimeio_affine")
