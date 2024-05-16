if call_type == "on_questwork" then
    if mapid == 9000 and packet.skit_name == "skit_set_questwork_57" then
        local data = {}
        data.skit_name = "skit_set_questwork_57"
		local packet = {}
		packet.SkitItemAddResponse = data
        send(player,packet)
        move_player(player, 150)
    end
end