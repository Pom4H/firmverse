#include "board.h"

typedef uint16_t (*process_event_fn)(uint8_t, uint16_t);
typedef void (*reverse_bytes_fn)(uint8_t *, uint8_t);
typedef uint8_t (*ll_reset_fn)(void);
typedef uint8_t (*ll_set_adv_param_fn)(uint16_t, uint16_t, uint8_t, uint8_t, uint8_t, uint8_t *, uint8_t, uint8_t);
typedef uint8_t (*ll_set_data_length_fn)(uint16_t, uint16_t, uint16_t);

#define ROM_HCI_PROCESS_EVENT ((process_event_fn)0x000024FDu)
#define ROM_HCI_REVERSE_BYTES ((reverse_bytes_fn)0x000026A9u)
#define ROM_LL_PROCESS_EVENT ((process_event_fn)0x000059F1u)
#define ROM_LL_RESET0 ((ll_reset_fn)0x00006609u)
#define ROM_LL_SET_ADV_PARAM0 ((ll_set_adv_param_fn)0x00006A9Du)
#define ROM_LL_SET_DATA_LENGTH0 ((ll_set_data_length_fn)0x00006E11u)

#define LL_STATUS_SUCCESS 0x00u
#define LL_STATUS_INACTIVE_CONNECTION 0x02u

int test_controller_abi(void)
{
    uint8_t bytes[6] = {0, 1, 2, 3, 4, 5};
    uint8_t direct_addr[6] = {0};

    ROM_HCI_REVERSE_BYTES(bytes, sizeof(bytes));
    if (bytes[0] != 5u || bytes[1] != 4u || bytes[2] != 3u ||
        bytes[3] != 2u || bytes[4] != 1u || bytes[5] != 0u) {
        return 0;
    }

    if (ROM_HCI_PROCESS_EVENT(1u, 0x1234u) != 0u) {
        return 0;
    }
    if (ROM_LL_PROCESS_EVENT(2u, 0x4321u) != 0u) {
        return 0;
    }

    if (ROM_LL_SET_ADV_PARAM0(0x20u, 0x40u, 0u, 0u, 0u,
                              direct_addr, 0x07u, 0u) != LL_STATUS_SUCCESS) {
        return 0;
    }

    /* The freestanding capability image has no host connection. Verify that
       direct legacy LL DLE reports the public inactive-connection status. */
    if (ROM_LL_SET_DATA_LENGTH0(0u, 251u, 2120u) != LL_STATUS_INACTIVE_CONNECTION) {
        return 0;
    }

    if (ROM_LL_RESET0() != LL_STATUS_SUCCESS) {
        return 0;
    }
    return 1;
}
