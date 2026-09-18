/* The pinned upstream runtime and host share a translation unit so observation
 * can use its private renderer without adding a second scan or modifying upstream. */
#include "../../third_party/fbd-runtime/fbdrt.c"
#include "saturn_bridge.c"
