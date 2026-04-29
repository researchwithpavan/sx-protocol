from sx import SxMessage

msg = SxMessage.parse_text('{name:"Asha"}')
print(msg.to_text())
msg.close()
