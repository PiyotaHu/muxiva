#include "voxa/rtc_adapter_v1.h"
#include <stddef.h>
_Static_assert(offsetof(voxa_rtc_adapter_config_v1, abi_version) == 0, "ABI prefix drift");
_Static_assert(offsetof(voxa_rtc_callbacks_v1, struct_size) == sizeof(uint32_t), "ABI size drift");
int main(void) { return VOXA_RTC_CREATED == 1 ? 0 : 1; }
