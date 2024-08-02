if call_type == "on_map_loaded" then
    if zone == "landing" then
        set_account_flag(sender, 93, 1)
        move_player(sender, "cutscene")
    elseif zone == "cutscene" then
        local cutscene_data = {}
        cutscene_data.scene_name = "st_010120_om"
        cutscene_data.unk5 = 6
        cutscene_data.unk7 = 14
        local packet = {}
        packet.StartCutscene = cutscene_data
        send(sender,packet)
    end
end

