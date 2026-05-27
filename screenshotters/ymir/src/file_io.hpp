#pragma once

#include <cstdint>
#include <filesystem>
#include <vector>

namespace yscreen {

bool read_file(const std::filesystem::path& path, std::vector<std::uint8_t>& out);

}
