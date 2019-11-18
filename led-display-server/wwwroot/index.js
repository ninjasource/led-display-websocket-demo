var nicknameInput = document.querySelector('input#nickname');
var errorLabel = document.querySelector('label#error');
var messageInput = document.querySelector('input#message');
// div sections
var landingPageSection = document.querySelector('div#landingPageSection');
var chatSection = document.querySelector('div#chatSection');
function setSectionVisible(section) {
    // set all sections to invisible
    landingPageSection.style.display = "none";
    chatSection.style.display = "none";
    // set the section below to visible
    section.style.display = "flex";
}
var WsSignaller = /** @class */ (function () {
    function WsSignaller(nickname) {
        this.nickname = nickname;
    }
    WsSignaller.prototype.connect = function () {
        var _this = this;
        var wsUri = (window.location.protocol === 'https:' && 'wss://' || 'ws://') + window.location.host + '/ws/' + this.nickname;
        console.log('ws connecting to: ' + wsUri);
        var wsConnection = new WebSocket(wsUri);
        this.wsConnection = wsConnection;
        wsConnection.onopen = this.onopen;
        wsConnection.onmessage = function (e) { return _this.onmessage(e); };
        wsConnection.onclose = function (e) { return console.log('ws disconnected'); };
    };
    WsSignaller.prototype.reconnect = function () {
        if (this.wsConnection != null && this.wsConnection.readyState == WebSocket.CLOSED) {
            console.log("reconnecting ws");
            this.connect();
        }
    };
    WsSignaller.prototype.send = function (msg) {
        if (this.wsConnection != null && this.wsConnection.readyState == WebSocket.OPEN) {
            this.wsConnection.send(msg);
        }
    };
    WsSignaller.prototype.onopen = function (e) {
        var _wsConnection = e.currentTarget;
        console.log('ws connected');
        setSectionVisible(chatSection);
    };
    WsSignaller.prototype.onmessage = function (e) {
        console.log('ws received: ' + e.data);
    };
    return WsSignaller;
}());
var ws = new WsSignaller("nobody");
function connectClick() {
    var nickname = nicknameInput.value;
    console.log(nickname);
    if (nickname == null || nickname.length < 3 || nickname.length > 10) {
        errorLabel.innerHTML = "Please enter a valid nickname<br>between 3 and 10 chars";
        return;
    }
    errorLabel.innerHTML = null;
    ws = new WsSignaller(nickname);
    ws.connect();
}
function messageChanged() {
    var msg = messageInput.value;
    console.log(msg);
    if (msg == null || msg.length == 0 || msg.length > 100) {
        // log out of range
        return;
    }
    ws.send(msg);
    messageInput.value = null;
}
window.onfocus = function () {
    ws.reconnect();
};
