#include "ipl_picker.hpp"
#include "worker_driver.hpp"

#include <ymir/media/loader/loader.hpp>
#include <ymir/media/saturn_header.hpp>

#include <fmt/format.h>

#include <filesystem>
#include <string>
#include <string_view>

namespace {

void print_usage(const char* arg0) {
    fmt::print(stderr,
               "usage: {} (--ipl <file> | --ipl-dir <dir>) --disc <path> --out <dir>\n",
               arg0);
}

bool parse_arg(int& i, int argc, char** argv, std::string_view flag, std::string& dst) {
    if (std::string_view{argv[i]} != flag)
        return false;
    if (i + 1 >= argc)
        return false;
    dst = argv[++i];
    return true;
}

bool probe_area_codes(const std::filesystem::path& disc_path, ymir::media::AreaCode& out) {
    ymir::media::Disc disc;
    auto noop_msg = [](ymir::media::MessageType, std::string) {};
    if (!ymir::media::LoadDisc(disc_path, disc, false, noop_msg))
        return false;
    if (disc.header.hwID != ymir::media::SaturnHeader::kExpectedHwId)
        return false;
    out = disc.header.compatAreaCode;
    return true;
}

}

int main(int argc, char** argv) {
    std::string ipl_str, ipl_dir_str, disc_str, out_str;

    for (int i = 1; i < argc; ++i) {
        if (parse_arg(i, argc, argv, "--ipl", ipl_str))
            continue;
        if (parse_arg(i, argc, argv, "--ipl-dir", ipl_dir_str))
            continue;
        if (parse_arg(i, argc, argv, "--disc", disc_str))
            continue;
        if (parse_arg(i, argc, argv, "--out", out_str))
            continue;

        fmt::print(stderr, "unknown argument: {}\n", argv[i]);
        print_usage(argv[0]);
        return 2;
    }

    if (disc_str.empty() || out_str.empty() || (ipl_str.empty() && ipl_dir_str.empty())) {
        print_usage(argv[0]);
        return 2;
    }

    std::filesystem::path ipl_path;
    if (!ipl_str.empty()) {
        ipl_path = ipl_str;
    } else {
        auto catalog = yscreen::scan_ipl_dir(ipl_dir_str);
        if (catalog.empty()) {
            fmt::print(stderr, "no usable IPL ROMs found in '{}'\n", ipl_dir_str);
            return 1;
        }
        ymir::media::AreaCode area = ymir::media::AreaCode::None;
        if (!probe_area_codes(disc_str, area)) {
            fmt::print(stderr, "could not read Saturn header from '{}'\n", disc_str);
            return 1;
        }
        const auto* ipl = catalog.pick(area);
        if (!ipl) {
            fmt::print(stderr, "no IPL available for disc '{}'\n", disc_str);
            return 1;
        }
        ipl_path = ipl->path;
        fmt::print(stderr, "[worker] auto-selected IPL: {}\n", ipl_path.filename().string());
    }

    yscreen::WorkerOptions opts;
    opts.ipl_path = std::move(ipl_path);
    opts.disc_path = disc_str;
    opts.out_dir = out_str;

    std::error_code ec;
    std::filesystem::create_directories(opts.out_dir, ec);
    if (ec) {
        fmt::print(stderr, "failed to create out dir '{}': {}\n", opts.out_dir.string(), ec.message());
        return 1;
    }

    fmt::print(stderr, "[worker] ipl: {}\n", opts.ipl_path.string());
    fmt::print(stderr, "[worker] disc: {}\n", opts.disc_path.string());
    fmt::print(stderr, "[worker] out: {}\n", opts.out_dir.string());

    auto result = yscreen::run_worker(opts);
    if (!result.ok) {
        fmt::print(stderr, "[worker] failed: {}\n", result.error);
        return 1;
    }

    fmt::print(stderr, "[worker] game_id={} title=\"{}\" frames={}\n", result.game_id, result.game_title,
               result.frames_written);
    return 0;
}
