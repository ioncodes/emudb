#include "ipl_picker.hpp"

#include "file_io.hpp"

#include <ymir/core/hash.hpp>
#include <ymir/media/saturn_header.hpp>
#include <ymir/sys/memory_defs.hpp>
#include <ymir/util/bitmask_enum.hpp>

#include <fmt/format.h>

#include <array>
#include <vector>

namespace yscreen {

void IplCatalog::add(IplEntry entry) {
    by_region_.try_emplace(entry.region, std::move(entry));
}

const IplEntry* IplCatalog::pick(ymir::media::AreaCode area_codes) const {
    if (by_region_.empty())
        return nullptr;

    using ymir::db::SystemRegion;

    auto find = [&](SystemRegion r) -> const IplEntry* {
        auto it = by_region_.find(r);
        return (it != by_region_.end()) ? &it->second : nullptr;
    };

    const bool jp = BitmaskEnum(area_codes).AnyOf(ymir::media::AreaCode::Japan);

    const std::array<SystemRegion, 2> order =
        jp ? std::array{SystemRegion::JP, SystemRegion::US_EU} : std::array{SystemRegion::US_EU, SystemRegion::JP};

    // Prefer an exact region match; fall back to any region-free IPL before the other region.
    if (const auto* p = find(order[0]))
        return p;

    for (const auto& [region, entry] : by_region_) {
        if (entry.region_free)
            return &entry;
    }

    if (const auto* p = find(order[1]))
        return p;

    return &by_region_.begin()->second;
}

IplCatalog scan_ipl_dir(const std::filesystem::path& dir) {
    IplCatalog cat;
    if (!std::filesystem::is_directory(dir)) {
        fmt::print(stderr, "warning: IPL directory '{}' does not exist\n", dir.string());
        return cat;
    }

    for (const auto& e : std::filesystem::directory_iterator(dir)) {
        if (!e.is_regular_file())
            continue;

        std::vector<std::uint8_t> data;
        if (!read_file(e.path(), data))
            continue;
        if (data.size() != ymir::sys::kIPLSize)
            continue;

        const auto hash = ymir::CalcHash128(data.data(), data.size(), ymir::sys::kIPLHashSeed);
        const auto* info = ymir::db::GetIPLROMInfo(hash);
        if (!info) {
            fmt::print(stderr, "warning: unknown IPL '{}' (hash {})\n", e.path().filename().string(),
                       ymir::ToString(hash));
            continue;
        }

        cat.add(IplEntry{e.path(), info->version, info->region, info->regionFree});
    }

    return cat;
}

}
