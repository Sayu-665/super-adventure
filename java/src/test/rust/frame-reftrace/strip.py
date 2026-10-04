"""Reduce a CompiledPack JSON to its pipeline structure (pass order, flip schedule, targets,
binding names, program kinds and attachments). Removes everything taken from the pack's files:
options/lang, id maps, custom uniform expressions, custom texture paths, uniform layouts,
diagnostics, stage modules and blob references."""
import json, sys

def empty_like(v):
    if isinstance(v, list): return []
    if isinstance(v, dict): return {}
    return v

src, dst = sys.argv[1], sys.argv[2]
d = json.load(open(src))
d['options'] = {k: empty_like(v) for k, v in d['options'].items()}
d['id_maps'] = {k: empty_like(v) for k, v in d['id_maps'].items()}
d['diagnostics'] = []
d['blobs'] = []
d['info']['environment']['extra_macros'] = {}
for dim in d['dimensions']:
    dim['custom_uniforms'] = []
    dim['settings']['raw'] = {}
    for block in ('frame', 'draw'):
        dim['uniforms'][block]['members'] = []
    t = dim['targets']
    t['custom_textures'] = []
    t['images'] = []
    t['buffers'] = []
    t['noise_texture'] = None
    for p in dim['programs']:
        p['stages'] = []
        p['vertex_inputs'] = []
json.dump(d, open(dst, 'w'), separators=(',', ':'))
