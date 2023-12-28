if call_type == "interaction" then
	if packet.action == "Transfer" then
		-- get extra data for initial teleporter
		local result_err = {pcall(get_extra_data, packet.object1.id)}
		if result_err[1] == false then
			-- fallback if we couldn't find extra data
			result_err[2] = {}
			result_err[2].TransferTo = packet.object1.id
			print("Could not find object extra data for ", packet.object1.id)
		end
		local transfer_target = result_err[2].TransferTo
		assert(transfer_target ~= nil, "TransferTo not found")
		
		-- get target teleporter position
		local result = get_object(transfer_target)
		local pos = result.position
		
		-- prepare TeleportTransfer packet
		local transfer_packet_data = {}
		transfer_packet_data.location = pos
		transfer_packet_data.source_tele = packet.object1
		local transfer_packet = {}
		transfer_packet.TeleportTransfer = transfer_packet_data
		send(sender, transfer_packet)
		
		-- prepare forward SetTag packet
		local set_tag_data = {}
		-- receiver
		local receiver = {}
		receiver.id = 0
		receiver.entity_type = "Player"
		set_tag_data.object1 = receiver
		-- source tele
		set_tag_data.object2 = packet.object1
		-- target?
		set_tag_data.object3 = packet.object3
		set_tag_data.attribute = "Forwarded"
		local set_tag_fwd = {}
		set_tag_fwd.SetTag = set_tag_data
		
		-- send to players transfer info
		for i, user in ipairs(players) do
			set_tag_fwd.SetTag.object1.id = user
			send(user, set_tag_fwd)
		end
		
		-- prepare notification SetTag packet
		local set_tag = set_tag_fwd
		packet.object1.id = transfer_target
		-- receiver
		set_tag.SetTag.object1 = packet.object3
		-- actor?
		set_tag.SetTag.object2 = packet.object3
		-- target?
		set_tag.SetTag.object3 = packet.object1
		set_tag.SetTag.attribute = "ObjectTransfer"
		send(sender, set_tag)	
	end
elseif call_type == "to_vita" then
	for i=1,size,2 do
		if data[i] > 50 and data[i] < 80 then
			data[i] = data[i] - 1
		end
	end
end
