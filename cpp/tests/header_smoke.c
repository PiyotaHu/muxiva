#include "voxa/voxa.h"

int main(void) {
  voxa_runtime_v1 runtime = {0, 0};
  return (sizeof(runtime) == 16 && voxa_abi_version_v1() == VOXA_ABI_VERSION_V1) ? 0 : 1;
}
