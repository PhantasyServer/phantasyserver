if call_type == "on_cutscene_end" then
    if zone == "cutscene" then
        move_player(sender, "quest_select")
    end
end

