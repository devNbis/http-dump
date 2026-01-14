const host= window.location.host;
document.getElementById("host").append(host);
const socket = new WebSocket('ws://'+host+'/ws');
socket.addEventListener('message', e => {
    var item=document.createElement("li");
    item.setAttribute("class",e.data.startsWith("Request")?"req":e.data.includes("status: 20")?"res":"res_err");
    item.append(e.data);
    document.getElementById("messages").firstElementChild.before( item);
});
