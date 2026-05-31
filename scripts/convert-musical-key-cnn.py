"""Export KeyNet to ONNX for Rust inference.

Uses the legacy TorchScript tracer (dynamo=False) to avoid embedding
Python stack traces and personal file paths in the ONNX graph.  This
keeps the model self-contained and portable.
"""

import torch
from model import KeyNet

model = KeyNet(num_classes=24, in_channels=1, Nf=20)
model.load_state_dict(torch.load("checkpoints/keynet.pt", map_location="cpu"))
model.eval()

# Dummy input: (batch, channel, freq, time)
# Use a representative time dimension (~20s of audio at hop=8820 -> ~100 frames)
dummy_input = torch.randn(1, 1, 105, 100)

torch.onnx.export(
    model,
    dummy_input,
    "keynet.onnx",
    input_names=["input"],
    output_names=["logits"],
    dynamic_axes={
        "input": {3: "time"},  # Variable time dimension
        "logits": {0: "batch"},
    },
    opset_version=20,  # PyTorch 2.12 default; supported by ONNX Runtime 1.24+
    export_params=True,  # Inline weights (no external .data file)
    do_constant_folding=True,  # Fold constant operations
    dynamo=False,  # Legacy tracer — no stack traces in output
)

print("Exported keynet.onnx successfully.")
