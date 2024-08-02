if call_type == "on_map_loaded" then
    if zone == "lobby" then
        local quest_abandon = {}
        quest_abandon.unk1 = 0
        quest_abandon.unk2 = 0
        quest_abandon.unk3 = 0
        local packet = {}
        packet.Unk0E1A = quest_abandon
        send(sender, packet)
        packet.Unk0E1A.unk1 = 3
        send(sender, packet)
        packet.Unk0E1A.unk1 = 4
        send(sender, packet)
    elseif zone == "cafe" then
        local cutscene_played = get_character_flag(9518)
        if cutscene_played == 0 then
            move_player(sender, "cafe_cutscene")
        end
    elseif zone == "cafe_cutscene" then
        local cutscene_data = {}
        cutscene_data.scene_name = "pr_043070"
        cutscene_data.unk5 = 7
        cutscene_data.unk7 = 7
        local packet = {}
        packet.StartCutscene = cutscene_data
        send(sender,packet)
    elseif zone == "casino" then
        local cutscene_played = get_character_flag(1999)
        if cutscene_played == 0 then
            move_player(sender, "casino_cutscene")
        end
    elseif zone == "casino_cutscene" then
        local cutscene_data = {}
        cutscene_data.scene_name = "un_039010"
        cutscene_data.unk5 = 6
        cutscene_data.unk7 = 7
        local packet = {}
        packet.StartCutscene = cutscene_data
        send(sender,packet)
    end
end
