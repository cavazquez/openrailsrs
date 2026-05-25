SIMISA@@@@@@@@@@JINX0s1t______
( shape
    ( shape_header 1 0 )
    ( volumes 1 ( vol_sphere ( vector 0 0 0 ) 1.0 ) )
    ( shader_names 1 "TexDiff" )
    ( texture_filenames 1 "yard.ace" )
    ( points 4
        ( point -40.0 1.0 -25.0 )
        ( point  40.0 1.0 -25.0 )
        ( point  40.0 1.0  25.0 )
        ( point -40.0 1.0  25.0 )
    )
    ( uv_points 4
        ( uv_point 0 0 )
        ( uv_point 1 0 )
        ( uv_point 1 1 )
        ( uv_point 0 1 )
    )
    ( normals 1
        ( vector 0 1 0 )
    )
    ( prim_states 1
        ( prim_state "yard_slab" 0 ( tex_idxs 1 0 ) )
    )
    ( lod_controls 1
        ( lod_control
            ( distance_levels_header )
            ( distance_levels 1
                ( distance_level
                    ( distance_level_header
                        ( dlevel_selection 2000 )
                    )
                    ( sub_objects 1
                        ( sub_object
                            ( vertices 4 )
                            ( primitives 1
                                ( prim_state_idx 0 )
                                ( indexed_trilist
                                    ( vertex_idxs 6 0 1 2 0 2 3 )
                                )
                            )
                        )
                    )
                )
            )
        )
    )
    ( matrices 1
        ( matrix "MAIN"
            1 0 0
            0 1 0
            0 0 1
            0 0 0
        )
    )
)
