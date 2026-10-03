"""Rewrite a `shaderbridge compile` output (pack.json + blobs.bin) keeping only SPIR-V blobs."""
import json, sys
src, dst = sys.argv[1], sys.argv[2]
m = json.load(open(f"{src}/pack.json"))
data = open(f"{src}/blobs.bin", "rb").read()
infos = m["blobs"]
remap, out, new_infos = {}, bytearray(), []
for i, b in enumerate(infos):
    if b["kind"] != "spirv":
        continue
    while len(out) % 8:
        out.append(0)
    remap[i] = len(new_infos)
    new_infos.append({"kind": "spirv", "offset": len(out), "len": b["len"]})
    out += data[b["offset"]:b["offset"] + b["len"]]
for d in m["dimensions"]:
    for p in d["programs"]:
        for s in p["stages"]:
            s["spirv"] = remap[s["spirv"]] if s.get("spirv") is not None else None
            s["glsl_vulkan"] = None
            s["glsl_renderpearl"] = None
m["blobs"] = new_infos
import os
os.makedirs(dst, exist_ok=True)
json.dump(m, open(f"{dst}/pack.json", "w"), separators=(",", ":"))
open(f"{dst}/blobs.bin", "wb").write(out)
print(len(new_infos), "spirv blobs,", len(out), "bytes")
