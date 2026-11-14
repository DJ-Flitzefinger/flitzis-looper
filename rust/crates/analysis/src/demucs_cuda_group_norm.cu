// Exact contiguous FP32 CUDA GroupNorm candidate for the pinned HTDemucs graph.
// Development compiler: checked ORT CUDA package NVRTC 12.8, --fmad=true,
// --prec-div=true, --ftz=false, --gpu-architecture=compute_120.

struct Moments {
    float mean;
    float m2;
    float count;
};

__device__ Moments combine(Moments a, Moments b) {
    if (a.count == 0.0f) {
        return b;
    }
    if (b.count == 0.0f) {
        return a;
    }
    const float delta = b.mean - a.mean;
    const float count = a.count + b.count;
    const float fraction = b.count / count;
    return {
        a.mean + delta * fraction,
        a.m2 + b.m2 + delta * delta * a.count * fraction,
        count,
    };
}

__device__ Moments warp_reduce(Moments value) {
    for (int hop = 16; hop > 0; hop /= 2) {
        const Moments peer = {
            __shfl_down_sync(0xffffffff, value.mean, hop),
            __shfl_down_sync(0xffffffff, value.m2, hop),
            __shfl_down_sync(0xffffffff, value.count, hop),
        };
        value = combine(value, peer);
    }
    return value;
}

extern "C" __global__ void htdemucs_group_norm(
    const float* input,
    const float* gamma,
    const float* beta,
    float* output,
    int channels,
    int spatial
) {
    // The checked release contains only one group. Each block processes all
    // channels of one sample, with independent warp reductions per row.
    const int size = channels * spatial;
    const int lane = threadIdx.x;
    const float* source = input + blockIdx.x * size;
    float* destination = output + blockIdx.x * size;
    Moments value = {0.0f, 0.0f, 0.0f};
    int observations = 0;
    for (int offset = lane; offset < size; offset += blockDim.x) {
        const float x = source[offset];
        const float delta = x - value.mean;
        const float mean = value.mean + delta / float(++observations);
        value.m2 += delta * (x - mean);
        value.mean = mean;
    }
    value.count = float(observations);
    value = warp_reduce(value);

    __shared__ Moments warps[16];
    __shared__ float mean, reciprocal_std;
    if (blockDim.x > 32) {
        if (lane % 32 == 0) {
            warps[lane / 32] = value;
        }
        __syncthreads();
        if (lane < 32) {
            const Moments moment = lane < blockDim.x / 32
                ? warps[lane]
                : Moments{0.0f, 0.0f, 0.0f};
            value = warp_reduce(moment);
        }
    }
    if (lane == 0) {
        mean = value.mean;
        reciprocal_std = rsqrtf(value.m2 / value.count + 1.0e-5f);
    }
    __syncthreads();

    for (int offset = lane; offset < size; offset += blockDim.x) {
        const int channel = offset / spatial;
        const float scale = reciprocal_std * gamma[channel];
        const float bias = __fmaf_rn(-scale, mean, beta[channel]);
        destination[offset] = __fmaf_rn(scale, source[offset], bias);
    }
}
