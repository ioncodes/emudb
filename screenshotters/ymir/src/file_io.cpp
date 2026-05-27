#include "file_io.hpp"

#include <fstream>
#include <ios>

namespace yscreen {

bool read_file(const std::filesystem::path& path, std::vector<std::uint8_t>& out) {
    std::ifstream f(path, std::ios::binary | std::ios::ate);
    if (!f)
        return false;

    auto sz = f.tellg();
    if (sz < 0)
        return false;
    f.seekg(0, std::ios::beg);

    out.resize(static_cast<size_t>(sz));
    if (sz > 0)
        f.read(reinterpret_cast<char*>(out.data()), sz);

    return f.good() || f.eof();
}

}
