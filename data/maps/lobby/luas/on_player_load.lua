if call_type == "on_player_load" then
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
end
