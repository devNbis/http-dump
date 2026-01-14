const host= window.location.host;
document.getElementById("host").append(host);

const socket = new WebSocket('ws://'+host+'/ws');

socket.addEventListener('message', e => {
    //document.getElementById("messages").append(e.data, document.createElement("li"));
    var item=document.createElement("li")
    item.append(e.data)
    document.getElementById("messages").firstElementChild.before( item);
});
/*
const form = document.querySelector("form");
form.addEventListener("submit", () => {
    socket.send(form.elements.namedItem("content").value);
    form.elements.namedItem("content").value = "";
});*/