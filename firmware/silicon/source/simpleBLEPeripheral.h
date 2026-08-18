#ifndef SIMPLEBLEPERIPHERAL_H
#define SIMPLEBLEPERIPHERAL_H

#ifdef __cplusplus
extern "C" {
#endif

#define SBP_START_DEVICE_EVT 0x0001
#define SBP_ENABLE_SCAN_EVT  0x0040
#define SBP_RANK_EVT         0x0080

void SimpleBLEPeripheral_Init(uint8 task_id);
uint16 SimpleBLEPeripheral_ProcessEvent(uint8 task_id, uint16 events);

#ifdef __cplusplus
}
#endif

#endif
