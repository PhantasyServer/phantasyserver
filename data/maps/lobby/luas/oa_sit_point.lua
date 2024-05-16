if call_type == "interaction" then
	if packet.action == "Sit" then
		local sit_packet_data = {}
		
		-- receiver
		local receiver = {}
		receiver.id = 0
		receiver.entity_type = "Player"
		sit_packet_data.receiver = receiver
		-- chair
		sit_packet_data.target = packet.object1
		-- target player
		sit_packet_data.object3 = packet.object3
		sit_packet_data.attribute = "SitSuccess"
		local sit_packet = {}
		sit_packet.SetTag = sit_packet_data
		
		-- send info to players
		for i, user in ipairs(players) do
			sit_packet.SetTag.receiver.id = user
			send(user, sit_packet)
		end
	end
end
