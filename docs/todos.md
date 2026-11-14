# Project TODOs

Record explicit user-requested work here. Work items require their own task
authorization and applicable [OpenSpec contracts](../openspec/specs/).

- [ ] **Complete the Torch replacement.** The repository has Native inference
  entry points and candidate components. The first cutover attempt failed, so the
  current Torch paths remain in use. Follow the
  [temporary replacement plan](plans/torch-replacement.md) for Beat This and
  both supported stem models. Verify supported NVIDIA CUDA, fresh same-model
  CPU fallback, complete source and quality parity, and the shipping packages
  before retiring Torch. Once verified, remove the plan, resolve this item and
  update the architecture and setup guides.
