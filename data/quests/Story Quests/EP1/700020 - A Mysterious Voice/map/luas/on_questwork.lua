if call_type == "on_questwork" then
    if zone == "cutscene" and packet.skit_name == "skit_set_questwork_30" then
        local data = {}
        data.skit_name = "skit_set_questwork_30"
		local packet = {}
		packet.SkitItemAddResponse = data
        send(sender,packet)
    end
end
