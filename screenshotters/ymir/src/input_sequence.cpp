#include "input_sequence.hpp"

namespace yscreen {

using ymir::peripheral::Button;

ymir::peripheral::Button deterministic_buttons(int iteration) {
    Button pressed = (iteration % 2 == 0) ? (Button::A | Button::Start) : Button::None;
    return Button::All & ~pressed;
}

}
