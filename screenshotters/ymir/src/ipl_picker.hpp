#pragma once

#include <ymir/db/ipl_db.hpp>
#include <ymir/media/media_defs.hpp>

#include <filesystem>
#include <map>
#include <string>

namespace yscreen {

struct IplEntry {
    std::filesystem::path path;
    std::string version;
    ymir::db::SystemRegion region;
};

class IplCatalog {
public:
    bool empty() const {
        return by_region_.empty();
    }
    size_t size() const {
        return by_region_.size();
    }

    void add(IplEntry entry);

    const IplEntry* pick(ymir::media::AreaCode area_codes) const;

    auto begin() const {
        return by_region_.begin();
    }
    auto end() const {
        return by_region_.end();
    }

private:
    std::map<ymir::db::SystemRegion, IplEntry> by_region_;
};

IplCatalog scan_ipl_dir(const std::filesystem::path& dir);

}
