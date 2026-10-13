#!/usr/bin/env python3
"""Regenerate owned MSTS meshes used by equipment acceptance, without author assets."""
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def body():
 return [
 ('MAIN',[[-1.5,.5,-10],[1.5,.5,-10],[1.5,3.4,-10],[-1.5,3.4,-10]]),
 ('MAIN',[[-1.5,.5,10],[-1.5,3.4,10],[1.5,3.4,10],[1.5,.5,10]]),
 ('MAIN',[[-1.5,.5,-10],[-1.5,3.4,-10],[-1.5,3.4,10],[-1.5,.5,10]]),
 ('MAIN',[[1.5,.5,-10],[1.5,.5,10],[1.5,3.4,10],[1.5,3.4,-10]]),
 ('MAIN',[[-1.5,3.4,-10],[1.5,3.4,-10],[1.5,3.4,10],[-1.5,3.4,10]]),
 ('MAIN',[[-1.5,.5,-10],[-1.5,.5,10],[1.5,.5,10],[1.5,.5,-10]])]
def shape(path,parts,animation,texture):
 names=list(dict.fromkeys(n for n,_ in parts));points=[p for _,v in parts for p in v]
 s='SIMISA@@@@@@@@@@JINX0s1t______\n(shape (shape_header 1 0) (shader_names 1 "TexDiff") (texture_filenames 1 "'+texture+'") (textures 1 (texture 0 0 0 0))\n'
 s+=f'(points {len(points)} '+''.join('(point '+' '.join(map(str,p))+')' for p in points)+')\n'
 s+='(uv_points 4 (uv_point 0 0) (uv_point 1 0) (uv_point 1 1) (uv_point 0 1)) (normals 1 (vector 0 1 0))\n'
 count=len(names)
 s+=f'(matrices {count} '+''.join(f'(matrix "{n}" 1 0 0 0 1 0 0 0 1 0 0 0)' for n in names)+f') (vtx_states {count} '+''.join(f'(vtx_state 0 {i} -5 0)' for i in range(count))+')\n'
 s+=f'(prim_states {count} '+''.join(f'(prim_state "part{i}" 0 0 (tex_idxs 1 0) 0 {i} 0 0 1)' for i in range(count))+')\n'
 s+=f'(lod_controls 1 (lod_control (distance_levels_header) (distance_levels 1 (distance_level (distance_level_header (dlevel_selection 2000) (hierarchy {count} -1 '+('0 '*(count-1))+')) (sub_objects '+str(len(parts))+' '
 for i,(name,vertices) in enumerate(parts):
  m=names.index(name)
  s+='(sub_object (vertices 4 '+''.join(f'(vertex {m} {i*4+j} 0 (vertex_uvs 1 {j}))' for j in range(4))+f') (primitives 1 (prim_state_idx {m}) (indexed_trilist (vertex_idxs 6 0 1 2 0 2 3))))'
 s+=')))))\n'+f'(animations 1 (animation 17 16 (anim_nodes {count} '
 for n in names:
  if n in animation:
   delta=' '.join(map(str,animation[n]));s+=f'(anim_node "{n}" (controllers 1 (linear_pos (linear_key 0 0 0 0) (linear_key 16 {delta}))))'
  else:s+=f'(anim_node "{n}" (controllers 0))'
 s+=')))\n)\n';path.write_text(s)
panto=body()
for i,z in enumerate([-8.,8.],1):panto.append((f'PANTOGRAPH{i}',[[-1.,3.6,z-.25],[1.,3.6,z-.25],[1.,3.6,z+.25],[-1.,3.6,z+.25]]))
shape(ROOT/'examples/electric_supply/vehicles/two_pantographs.s',panto,{'PANTOGRAPH1':[0,1.7,0],'PANTOGRAPH2':[0,1.7,0]},'../../player_lab/vehicles/lab.ace')
coach=body()+[('DOOR_D',[[-1.52,.5,-8],[-1.52,2.9,-8],[-1.52,2.9,-6],[-1.52,.5,-6]]),('DOOR_A',[[1.52,.5,-8],[1.52,.5,-6],[1.52,2.9,-6],[1.52,2.9,-8]])]
shape(ROOT/'examples/player_lab/vehicles/coach.s',coach,{'DOOR_D':[0,0,1.5],'DOOR_A':[0,0,-1.5]},'lab.ace')

# Reuse only the repository's owned terrain for playable equipment laboratories.
# Their graph, electrical sectors and rolling stock remain scenario-specific.
import shutil
for name, electrified in [('electric_supply', 'On'), ('traction_operation', 'Off')]:
 dst = ROOT/'examples'/name/'route'
 for folder in ['TERRAIN', 'TERRTEX', 'WORLD', 'SHAPES', 'TEXTURES']:
  shutil.copytree(ROOT/'examples/player_lab/route'/folder, dst/folder, dirs_exist_ok=True)
 (dst/'equipment.trk').write_text(
  'SIMISA@@@@@@@@@@JINX0r0t______\nTr_RouteFile ( RouteID ( "equipment_'+name+'" ) '
  'Name ( "Laboratorio propio de equipos" ) Electrified ( '+electrified+' ) '
  'MaxLineVoltage ( 25000 ) OverheadWireHeight ( 5.3 ) )\n')
