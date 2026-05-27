#include "png_writer.hpp"

#define STB_IMAGE_WRITE_IMPLEMENTATION
#define STBI_WRITE_NO_STDIO
#include "third_party/stb_image_write.h"

#include <cstdio>
#include <vector>

namespace yscreen {

namespace {

    void stbi_write_callback(void* context, void* data, int size) {
        auto* fp = static_cast<std::FILE*>(context);
        std::fwrite(data, 1, static_cast<size_t>(size), fp);
    }

}

bool write_png(const std::filesystem::path& path, const std::uint32_t* fb, std::uint32_t width, std::uint32_t height) {
    if (width == 0 || height == 0 || fb == nullptr)
        return false;

    std::vector<std::uint8_t> rgba(static_cast<size_t>(width) * height * 4);
    for (std::uint32_t i = 0; i < width * height; ++i) {
        const std::uint32_t px = fb[i];
        rgba[i * 4 + 0] = static_cast<std::uint8_t>((px >> 0) & 0xFF);
        rgba[i * 4 + 1] = static_cast<std::uint8_t>((px >> 8) & 0xFF);
        rgba[i * 4 + 2] = static_cast<std::uint8_t>((px >> 16) & 0xFF);
        rgba[i * 4 + 3] = 0xFF;
    }

    std::FILE* fp = std::fopen(path.string().c_str(), "wb");
    if (!fp)
        return false;

    int ok = stbi_write_png_to_func(stbi_write_callback, fp, static_cast<int>(width), static_cast<int>(height), 4,
                                    rgba.data(), static_cast<int>(width * 4));
    std::fclose(fp);
    return ok != 0;
}

}
