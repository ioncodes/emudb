#include "game_id.hpp"

#include <cctype>

namespace yscreen {

std::string normalize_game_id(std::string_view product_number) {
    size_t begin = 0;
    size_t end = product_number.size();

    while (begin < end && std::isspace(static_cast<unsigned char>(product_number[begin])))
        ++begin;
    while (end > begin && std::isspace(static_cast<unsigned char>(product_number[end - 1])))
        --end;
    if (begin == end)
        return {};

    std::string out;
    out.reserve(end - begin);

    for (size_t i = begin; i < end; ++i) {
        unsigned char c = static_cast<unsigned char>(product_number[i]);
        if (std::isalnum(c) || c == '-' || c == '_') {
            out.push_back(static_cast<char>(std::toupper(c)));
        } else {
            out.push_back('_');
        }
    }
    return out;
}

}
