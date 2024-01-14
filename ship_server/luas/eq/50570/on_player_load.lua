if call_type == "on_player_load" then
    if mapid == 9000 then
        local cutscene_data = {}
        cutscene_data.scene_name = "mv_010270"
        cutscene_data.unk5 = 6
        cutscene_data.unk7 = 2
		local packet = {}
		packet.StartCutscene = cutscene_data
        send(player,packet)
    end
end