# The Audio Unit (AUv2) wrapper around an app's CLAP (packaging/auv2/build.sh
# sets the variables).
cmake_minimum_required(VERSION 3.21)
project(dreist-auv2 LANGUAGES C CXX OBJC OBJCXX)

set(CMAKE_CXX_STANDARD 17)
add_subdirectory("${CLAP_WRAPPER_ROOT}" clap-wrapper EXCLUDE_FROM_ALL)

add_library(${AU_NAME}_auv2 MODULE)
target_add_auv2_wrapper(
    TARGET ${AU_NAME}_auv2
    OUTPUT_NAME "${AU_NAME}"
    BUNDLE_IDENTIFIER "${AU_BUNDLE_ID}"
    BUNDLE_VERSION "${AU_VERSION}"
    MANUFACTURER_NAME "${AU_MANUFACTURER_NAME}"
    MANUFACTURER_CODE "${AU_MANUFACTURER}"
    SUBTYPE_CODE "${AU_SUBTYPE}"
    INSTRUMENT_TYPE "aumu"
    MACOS_EMBEDDED_CLAP_LOCATION "${AU_CLAP}"
)
set_target_properties(${AU_NAME}_auv2 PROPERTIES LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}")
