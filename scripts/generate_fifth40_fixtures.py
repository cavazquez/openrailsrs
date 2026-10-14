#!/usr/bin/env python3
"""Small owned shapes for gangway, hose, cable, coupler and Jacobs acceptance."""
from pathlib import Path
import math
import shutil
from generate_equipment_fixtures import body, shape
ROOT=Path(__file__).resolve().parents[1]
def main():
    folder=ROOT/'examples/player_lab/vehicles'
    parts=body()
    for name,x,y in [('GANGWAY',0.,2.0),('AIR_HOSE',.85,.6),('ELECTRIC_CABLE',-.85,.7),('COUPLER',0.,.4)]:
        width=.65 if name=='GANGWAY' else .05
        height=.7 if name=='GANGWAY' else .03
        parts.extend([
            (name,[[x-width,y-height,-9.8],[x-width,y+height,-9.8],[x-width,y+height,-10.2],[x-width,y-height,-10.2]]),
            (name,[[x+width,y-height,-9.8],[x+width,y-height,-10.2],[x+width,y+height,-10.2],[x+width,y+height,-9.8]])])
    parts.extend([('BOGIE2',[[-1.1,.2,-1.1],[1.1,.2,-1.1],[1.1,.7,1.1],[-1.1,.7,1.1]])])
    # Authored rotating axle meshes provide real wheel/contact trajectories for
    # snow-clearance acceptance; no fabricated marker path is stamped.
    for wheel in ['WHEELS1','WHEELS2']:
        parts.append((wheel,[[-.85,-.3,0.],[.85,-.3,0.],[.85,.3,0.],[-.85,.3,0.]]))
    path=folder/'connections.s'
    shape(path,parts,{},'lab.ace')
    text=path.read_text().replace('(matrix "BOGIE2" 1 0 0 0 1 0 0 0 1 0 0 0)','(matrix "BOGIE2" 1 0 0 0 1 0 0 0 1 0 0 -9.8)')
    for wheel,z in [('WHEELS1',-7.),('WHEELS2',7.)]:
        text=text.replace(f'(matrix "{wheel}" 1 0 0 0 1 0 0 0 1 0 0 0)',f'(matrix "{wheel}" 1 0 0 0 1 0 0 0 1 0 .45 {z})')
    path.write_text(text)
    (folder/'connections.eng').write_text((folder/'lab.eng').read_text().replace('WagonShape ( lab.s )','WagonShape ( connections.s )'))
    (ROOT/'examples/player_lab/consists/connections.con').write_text('(Train (Engine "vehicles/connections.eng") (Wagon "vehicles/lab.wag"))\n')
    source=(ROOT/'examples/player_lab/scenario.toml').read_text().replace('consists/lab.con','consists/connections.con')
    source=source.replace('Laboratorio de cabina y carteles','Laboratorio de fuelles, mangueras y acopladores')
    # Place declarations before the next vehicle presentation table.
    index=source.index('[[train.presentation]]\nvehicle = 1')
    declarations=''
    for matrix,kind,x,y in [('GANGWAY','gangway',0.,2.),('AIR_HOSE','air_hose',.85,.6),('ELECTRIC_CABLE','electric_cable',-.85,.7),('COUPLER','coupler',0.,.4)]:
        declarations += f'[[train.presentation.connections]]\nmatrix="{matrix}"\nneighbor=1\nown_m=[{x},{y},9.8]\nrest_far_m=[{x},{y},10.2]\nother_m=[{x},{y},-9.8]\nkind="{kind}"\nmaximum_span_m=2.0\n'
    source=source[:index]+declarations+source[index:]
    (ROOT/'examples/player_lab/scenario_connections.toml').write_text(source)
    index=source.index('[[train.presentation]]\nvehicle = 1')
    source=source[:index]+'[train.presentation.jacobs]\nneighbor=1\nown_support_m=10.0\nother_support_m=-10.0\nbogie_matrix="BOGIE2"\n'+source[index:]
    source=source.replace('Laboratorio de fuelles, mangueras y acopladores','Laboratorio de bogie compartido Jacobs')
    (ROOT/'examples/player_lab/scenario_jacobs.toml').write_text(source)
    # An opaque closed wall, distinct from alpha-tested window cards, exercises
    # the actual exterior-eye collision volume.
    wall=[]
    for name,vertices in body():
        wall.append((name,[[x*2.,y, z*.05] for x,y,z in vertices]))
    shape(ROOT/'examples/player_lab/route/SHAPES/camera_wall.s',wall,{},'yard.ace')
    base_world=ROOT/'examples/player_lab/route/WORLD/w-000000-000000.w'
    world_text=base_world.read_text()
    if 'camera_wall.s' not in world_text:
        pos=world_text.rfind(')')
        world_text=world_text[:pos]+' ( Static ( UiD 23 ) ( FileName "camera_wall.s" ) ( Position 88.0 0.0 18.0 ) ( QDirection 0.0 0.0 0.0 1.0 ) )\n'+world_text[pos:]
        base_world.write_text(world_text)
    # Native-compatible industrial pickup arm and matching diesel intake.
    (folder/'refill.eng').write_text((folder/'connections.eng').read_text().replace('MaxBrakeForce ( 120kN )','MaxBrakeForce ( 120kN ) IntakePoint ( 0m 3m FuelDiesel )'))
    (ROOT/'examples/player_lab/consists/refill.con').write_text('(Train (Engine "vehicles/refill.eng") (Wagon "vehicles/lab.wag"))\n')
    refill=(ROOT/'examples/player_lab/scenario_connections.toml').read_text().replace('consists/connections.con','consists/refill.con').replace('Laboratorio de fuelles, mangueras y acopladores','Laboratorio de abastecimiento original')
    (ROOT/'examples/player_lab/scenario_refill.toml').write_text(refill)
    # Closed boxes remain visible from both sides of the installation. The
    # mast stands beside the car rather than disappearing inside its body.
    tower=[]
    for _,vertices in body():
        tower.append(('MAIN',[[x*.1333333333,(y-.5)*4.1/2.9,z*.02] for x,y,z in vertices]))
        tower.append(('ANIMATED_PARTS',[[x*.0666666667,4.1+(y-.5)*.2/2.9,(z+10.)*3.1/20.] for x,y,z in vertices]))
    shape(ROOT/'examples/player_lab/route/SHAPES/fuel_tower.s',tower,
          {'ANIMATED_PARTS':[0.,-1.8,0.]},'yard.ace')
    world_text=base_world.read_text()
    if 'fuel_tower.s' not in world_text:
        pos=world_text.rfind(')')
        world_text=world_text[:pos]+' ( Pickup ( UiD 30 ) ( FileName "fuel_tower.s" ) ( PickupType 7 0 ) ( PickupCapacity 100 1 ) ( PickupAnimData 0 4 ) ( SpeedRange 0 0 ) ( Position 70.0 0.0 -2.7 ) ( QDirection 0.0 0.0 0.0 1.0 ) )\n'+world_text[pos:]
    world_text=world_text.replace('( Position 70.0 0.0 -0.7 )','( Position 70.0 0.0 -2.7 )')
    base_world.write_text(world_text)
    route=ROOT/'examples/player_lab/curve_route';route.mkdir(exist_ok=True)
    for directory in ['TERRAIN','TERRTEX','WORLD','SHAPES','TEXTURES']:
        shutil.copytree(ROOT/'examples/player_lab/route'/directory,route/directory,dirs_exist_ok=True)
    world=route/'WORLD/w-000000-000000.w'
    text=world.read_text()
    text='\n'.join(line for line in text.splitlines() if 'yard_shed.s' not in line)+'\n'
    world.write_text(text)

    points=[(0.,0.),(100.,0.)]
    for step in range(1,37):
        angle=step*math.pi/36
        points.append((100.+180.*math.sin(angle),180.*(1-math.cos(angle))))
    points.append((-100.,360.))
    graph='[route]\nid="owned-curve-lab"\n'
    for i,(x,z) in enumerate(points):
        graph+=f'\n[[nodes]]\nid="n{i}"\nx_m={x:.6f}\ny_m={z:.6f}\n'
    for i,(a,b) in enumerate(zip(points,points[1:])):
        length=math.dist(a,b)
        for reverse in [False,True]:
            suffix='_r' if reverse else ''
            start,end=(i+1,i) if reverse else (i,i+1)
            graph+=f'\n[[edges]]\nid="c{i}{suffix}"\nfrom="n{start}"\nto="n{end}"\nlength_m={length:.6f}\nspeed_limit_kmh=25.0\ngrade_percent=0.0\n'
    (route/'track.toml').write_text(graph)
    for original,name in [('scenario_connections.toml','scenario_connections_curve.toml'),('scenario_jacobs.toml','scenario_jacobs_curve.toml')]:
        text=(ROOT/'examples/player_lab'/original).read_text().replace('path = "route"','path = "curve_route"').replace('start = "yard_a"','start = "n0"').replace('destination = "yard_b"',f'destination = "n{len(points)-1}"').replace('start_offset_m = 80.0','start_offset_m = 135.0')
        (ROOT/'examples/player_lab'/name).write_text(text)

if __name__=='__main__':main()
