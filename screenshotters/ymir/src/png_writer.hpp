#pragma once

#include <cstdint>
#include <filesystem>

namespace yscreen {

bool write_png(const std::filesystem::path& path, const std::uint32_t* fb, std::uint32_t width, std::uint32_t height);

}
