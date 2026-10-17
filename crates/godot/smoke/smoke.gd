# Smoke test for the Godot adapter. Run through run.sh, which passes one mode after "--".
#   play  plays a proof game to the end, then checks the wrong-thread guard
#   drop  drops the last reference to a session on a second thread
#   thread_owned  makes and uses a session on a second thread (recorded, not required)
extends SceneTree

const MAX_ANSWERS := 5000


func _initialize() -> void:
	var args := OS.get_cmdline_user_args()
	var mode := args[0] if args.size() > 0 else "play"
	var ok := false
	match mode:
		"play":
			ok = _play()
		"drop":
			ok = _drop()
		"thread_owned":
			ok = _thread_owned()
		_:
			printerr("unknown mode ", mode)
	print("SMOKE ", mode, " ", "PASS" if ok else "FAIL")
	quit(0 if ok else 1)


func _bytes(value: Dictionary) -> PackedByteArray:
	return JSON.stringify(value).to_utf8_buffer()


func _body(response: PackedByteArray) -> Dictionary:
	var parsed = JSON.parse_string(response.get_string_from_utf8())
	if typeof(parsed) != TYPE_DICTIONARY:
		return {}
	return parsed.get("body", {})


func _send(session, kind: String, seat, body: Dictionary) -> Dictionary:
	var request := {"v": 1, "kind": kind, "body": body}
	if seat != null:
		request["seat"] = seat
	return _body(session.handle(_bytes(request)))


# Passes priority, declines mulligans and takes the first legal pick, until the game ends.
func _play_game(session) -> Dictionary:
	var response := _send(session, "advance", 0, {})
	var answers := 0
	while response.get("status", "") != "game_over" and answers < MAX_ANSWERS:
		if response.get("status", "") != "decision_open":
			return {"ok": false, "why": "status " + str(response.get("status", "none"))}
		var decision: Dictionary = response["decision"]
		if decision["shape"] == "opaque":
			decision = _send(session, "poll", int(decision["player"]), {}).get("decision", {})
		var reply := {}
		match decision.get("shape", ""):
			"yes_no":
				reply = {"kind": "yes_no", "value": false}
			"action":
				reply = {"kind": "action", "action": {"kind": "pass_priority"}}
			"pick":
				reply = {"kind": "pick", "picks": decision["candidates"].slice(0, int(decision["min"]))}
			_:
				return {"ok": false, "why": "unhandled shape " + str(decision.get("shape", "none"))}
		response = _send(session, "answer", int(decision["player"]), {"decision": decision["id"], "reply": reply})
		answers += 1
	return {"ok": response.get("status", "") == "game_over", "answers": answers, "winner": response.get("winner")}


func _new_session():
	var session = StackfishSession.new()
	var created := _body(session.create(StackfishSession.proof_setup(2, 7)))
	if created.get("status", "") != "created":
		printerr("create returned ", created)
		return null
	return session


func _play() -> bool:
	var session = _new_session()
	if session == null:
		return false
	var result := _play_game(session)
	print("PLAYED ", result)
	if not result["ok"]:
		return false

	# A call from a second thread must not return a response and must leave the session usable.
	# With gdext's own safeguards on (the default), gdext panics first and GDScript gets null.
	# With them off, the adapter's guard answers with an empty array and "wrong_thread".
	# A call that fails inside gdext aborts the whole lambda, so each call gets its own thread.
	var first := Thread.new()
	first.start(func(): return session.handle(_bytes({"v": 1, "kind": "poll", "seat": 0, "body": {}})))
	var bytes = first.wait_to_finish()
	var last := Thread.new()
	last.start(func(): return session.last_error())
	var last_error = last.wait_to_finish()
	var refused: bool = bytes == null or (typeof(bytes) == TYPE_PACKED_BYTE_ARRAY and bytes.size() == 0)
	print("WRONG_THREAD handle=", "null (gdext refused)" if bytes == null else "empty (adapter refused)",
		" last_error=", "null (gdext refused)" if last_error == null else last_error)
	if not refused:
		return false
	var after := _send(session, "poll", 0, {})
	print("OWNER_AFTER status=", after.get("status", "none"), " last_error='", session.last_error(), "'")
	if after.get("status", "") != "game_over" or session.last_error() != "":
		return false

	return true


# Makes and uses a session on a second thread. With gdext's safeguards on, creating the object
# off the main thread aborts the process. Recorded, not required.
func _thread_owned() -> bool:
	var worker := Thread.new()
	worker.start(func():
		var own = _new_session()
		return own != null and _send(own, "advance", 0, {}).get("status", "") == "decision_open")
	var works: bool = worker.wait_to_finish()
	print("THREAD_OWNED works=", works)
	return works


func _drop() -> bool:
	var session = _new_session()
	if session == null:
		return false
	var before: int = StackfishSession.off_thread_drops()
	var holder := [session]
	session = null
	var dropper := Thread.new()
	dropper.start(func(): holder.clear())
	dropper.wait_to_finish()
	var after: int = StackfishSession.off_thread_drops()
	print("DROPPED holder_size=", holder.size(), " rust_drops_off_owner_thread=", after - before)
	return holder.size() == 0
