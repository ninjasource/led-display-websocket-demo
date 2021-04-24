const nicknameInput: HTMLInputElement = document.querySelector('input#nickname');
const errorLabel: HTMLLabelElement = document.querySelector('label#error');
const chatErrorLabel: HTMLLabelElement = document.querySelector('label#chatError');
const messageInput: HTMLInputElement = document.querySelector('input#message');


// div sections
const landingPageSection: HTMLElement = document.querySelector('div#landingPageSection');
const chatSection: HTMLElement = document.querySelector('div#chatSection');
//const errorSection: HTMLElement = document.querySelector('div#errorMessage');


function setSectionVisible(section: HTMLElement) {

    // set all sections to invisible
    landingPageSection.style.display = "none";
    chatSection.style.display = "none";

    // set the section below to visible
    section.style.display = "flex";
}

class WsSignaller {
    private wsConnection: WebSocket;
    private nickname: String;

    constructor(nickname: String) {
        this.nickname = nickname;
    }

    public connect() {
        let wsUri = (window.location.protocol === 'https:' && 'wss://' || 'ws://') + window.location.host + '/ws/' + this.nickname;
        console.log('ws connecting to: ' + wsUri);

        let wsConnection = new WebSocket(wsUri);
        this.wsConnection = wsConnection;

        wsConnection.onopen = this.onopen;
        wsConnection.onmessage = e => this.onmessage(e);
        wsConnection.onclose = e => console.log('ws disconnected');
    }

    public reconnect() {
        if (this.wsConnection != null && this.wsConnection.readyState == WebSocket.CLOSED) {
            console.log("reconnecting ws");
            this.connect();
            return true;
        }

        return false;
    }

    public send(msg: string) {
        if (this.wsConnection != null && this.wsConnection.readyState == WebSocket.OPEN) {
            this.wsConnection.send(msg);
        }
    }

    private onopen(e: Event) {
        let _wsConnection = e.currentTarget as WebSocket;
        console.log('ws connected');
        setSectionVisible(chatSection);

        // we may have a message to send if this was a reconnect
        messageChanged();
    }

    private onmessage(e: MessageEvent) {
        console.log('ws received: ' + e.data);
    }
}

let ws = new WsSignaller("nobody");

function isASCII(str) {
    return /^[\x00-\xFF]*$/.test(str);
}

function connectClick() {
    let nickname = nicknameInput.value;
    console.log(nickname);
    if (nickname == null || nickname.length < 3 || nickname.length > 10) {
        errorLabel.innerHTML = "Please enter a valid nickname<br>between 3 and 10 chars"
        return;
    }

    errorLabel.innerHTML = null;
    ws = new WsSignaller(nickname);
    ws.connect();
}

function messageChanged() {
    chatErrorLabel.innerHTML = null;
    let msg = messageInput.value;
    if (msg == null || msg.length == 0) {
        return;
    }

    console.log(msg);

    if (msg.length > 100) {
        chatErrorLabel.innerHTML = 'Please enter ASCII text less than 100 characters';
        return;
    }

    if (!isASCII(msg)) {
        chatErrorLabel.innerHTML = 'Please enter ASCII text as unicode cannot be rendered on the Led Display';
        return;
    }

    // reconnect if our browser has somehow disconnected (safari aka the new IExplorer)
    if (!ws.reconnect()) {
        // if we are already connected then send the message now
        ws.send(msg);
        messageInput.value = null;
    }
}

window.onfocus = () => {
    ws.reconnect();
}