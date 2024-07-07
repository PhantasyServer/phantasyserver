if call_type == "on_cutscene_end" then
    if zone == "cafe_cutscene" then
        move_player(sender, "cafe")
    elseif zone == "casino_cutscene" then
        move_player(sender, "casino")
    end
end

