#pragma once

#include <cstdint>
#include <filesystem>
#include <string>

namespace yscreen {

struct WorkerOptions {
    std::filesystem::path ipl_path;
    std::filesystem::path disc_path;
    std::filesystem::path out_dir;
    int iterations = 120;
    double seconds_per_iteration = 0.5;
    int screenshot_every_n_iters = 2;
    int input_start_frame = 300;
};

struct WorkerResult {
    bool ok = false;
    std::string game_id;
    std::string game_title;
    int frames_written = 0;
    std::string error;
};

WorkerResult run_worker(const WorkerOptions& opts);

}
