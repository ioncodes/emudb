#include "worker_driver.hpp"

#include "file_io.hpp"
#include "game_id.hpp"
#include "input_sequence.hpp"
#include "png_writer.hpp"

#include <ymir/hw/smpc/peripheral/peripheral_report.hpp>
#include <ymir/hw/smpc/peripheral/peripheral_state_common.hpp>
#include <ymir/media/loader/loader.hpp>
#include <ymir/sys/backup_ram.hpp>
#include <ymir/sys/memory_defs.hpp>
#include <ymir/sys/saturn.hpp>
#include <ymir/util/date_time.hpp>

#include <fmt/format.h>

#include <algorithm>
#include <cstdint>
#include <fstream>
#include <ios>
#include <span>
#include <vector>

namespace yscreen {

namespace {

    struct FrameBuffer {
        std::vector<std::uint32_t> pixels;
        std::uint32_t width = 0;
        std::uint32_t height = 0;
        bool fresh = false;
    };

    struct DriverState {
        FrameBuffer fb;
        ymir::peripheral::Button report_buttons = ymir::peripheral::Button::All;
    };

    void on_frame_complete(std::uint32_t* fb, std::uint32_t w, std::uint32_t h, void* ctx) {
        if (w == 0 || h == 0)
            return;

        auto* state = static_cast<DriverState*>(ctx);
        state->fb.pixels.assign(fb, fb + static_cast<size_t>(w) * h);
        state->fb.width = w;
        state->fb.height = h;
        state->fb.fresh = true;
    }

    void on_peripheral_report(ymir::peripheral::PeripheralReport& report, void* ctx) {
        auto* state = static_cast<DriverState*>(ctx);

        report.type = ymir::peripheral::PeripheralType::ControlPad;
        report.report.controlPad.buttons = state->report_buttons;
    }

    void install_smpc_bypass(ymir::Saturn& saturn) {
        const auto settings_path =
            std::filesystem::temp_directory_path() / fmt::format("ymir-shot-smpc-{}.bin", ::getpid());

        std::ofstream out(settings_path, std::ios::binary | std::ios::trunc);

        const std::uint8_t version[4] = {0x01, 0, 0, 0};
        const std::uint8_t smem[4] = {0, 0, 0, 0};
        const bool ste = true;
        const std::int64_t offset = 0;
        const std::int64_t timestamp = 0;

        out.write(reinterpret_cast<const char*>(version), sizeof(version));
        out.write(reinterpret_cast<const char*>(smem), sizeof(smem));
        out.write(reinterpret_cast<const char*>(&ste), sizeof(ste));
        out.write(reinterpret_cast<const char*>(&offset), sizeof(offset));
        out.write(reinterpret_cast<const char*>(&timestamp), sizeof(timestamp));
        out.close();

        std::error_code ec;
        saturn.SMPC.LoadPersistentDataFrom(settings_path, ec);
        std::filesystem::remove(settings_path, ec);
    }

    bool is_unicolor(const std::vector<std::uint32_t>& pixels) {
        if (pixels.empty())
            return true;

        const std::uint32_t first = pixels.front();
        for (auto px : pixels) {
            if (px != first)
                return false;
        }
        return true;
    }

}

WorkerResult run_worker(const WorkerOptions& opts) {
    WorkerResult result;

    std::vector<std::uint8_t> ipl;
    if (!read_file(opts.ipl_path, ipl)) {
        result.error = fmt::format("failed to read IPL '{}'", opts.ipl_path.string());
        return result;
    }
    if (ipl.size() != ymir::sys::kIPLSize) {
        result.error = fmt::format("IPL size mismatch: got {} bytes, expected {}", ipl.size(), ymir::sys::kIPLSize);
        return result;
    }

    ymir::media::Disc disc;
    auto noop_msg = [](ymir::media::MessageType, std::string) {};
    if (!ymir::media::LoadDisc(opts.disc_path, disc, true, noop_msg)) {
        result.error = fmt::format("failed to load disc '{}'", opts.disc_path.string());
        return result;
    }
    if (disc.header.hwID != ymir::media::SaturnHeader::kExpectedHwId) {
        result.error = "disc header is invalid (not a Saturn disc?)";
        return result;
    }

    result.game_id = normalize_game_id(disc.header.productNumber);
    result.game_title = disc.header.gameTitle;
    if (result.game_id.empty()) {
        result.error = "disc has empty product number; cannot derive game_id";
        return result;
    }

    auto saturn = std::make_unique<ymir::Saturn>();
    DriverState state;

    saturn->configuration.rtc.mode = ymir::core::config::rtc::Mode::Virtual;
    saturn->configuration.rtc.virtHardResetStrategy =
        ymir::core::config::rtc::HardResetStrategy::ResetToFixedTime;
    saturn->configuration.rtc.virtHardResetTimestamp = util::datetime::to_timestamp(
        util::datetime::DateTime{.year = 1994, .month = 1, .day = 1, .hour = 0, .minute = 0, .second = 0});

    saturn->LoadIPL(std::span<std::uint8_t, ymir::sys::kIPLSize>{ipl.data(), ymir::sys::kIPLSize});

    auto& port1 = saturn->SMPC.GetPeripheralPort1();
    port1.SetPeripheralReportCallback({&state, &on_peripheral_report});
    port1.ConnectControlPad();

    saturn->VDP.UseSoftwareRenderer();
    saturn->VDP.SetSoftwareRenderCallback({&state, &on_frame_complete});

    ymir::bup::BackupMemory bup;
    bup.CreateInMemory(ymir::sys::kInternalBackupRAMSize);
    saturn->mem.SetInternalBackupRAM(std::move(bup));

    install_smpc_bypass(*saturn);

    saturn->Reset(true);
    saturn->LoadDisc(std::move(disc));

    const int framerate = (saturn->GetVideoStandard() == ymir::core::config::sys::VideoStandard::PAL) ? 50 : 60;
    const int frames_per_iter = std::max(1, static_cast<int>(framerate * opts.seconds_per_iteration));

    int cumulative_frame = 0;

    for (int iter = 0; iter < opts.iterations; ++iter) {
        state.report_buttons = (cumulative_frame >= opts.input_start_frame)
                                   ? deterministic_buttons(iter)
                                   : ymir::peripheral::Button::All;

        for (int f = 0; f < frames_per_iter; ++f) {
            saturn->RunFrame();
            ++cumulative_frame;
        }

        const bool capture_this_iter =
            opts.screenshot_every_n_iters > 0 && (iter + 1) % opts.screenshot_every_n_iters == 0;

        if (!capture_this_iter || !state.fb.fresh)
            continue;
        state.fb.fresh = false;

        if (is_unicolor(state.fb.pixels))
            continue;

        const auto path = opts.out_dir / fmt::format("{}.png", cumulative_frame);
        if (write_png(path, state.fb.pixels.data(), state.fb.width, state.fb.height)) {
            ++result.frames_written;
        } else {
            fmt::print(stderr, "[{}] failed to write {}\n", result.game_id, path.string());
        }
    }

    result.ok = true;
    return result;
}

}
