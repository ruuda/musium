# Trådfri control

Musium can run a command before starting playback and after a period of
inactivity. In combination with [Ikea Trådfri wireless control outlets][outlets],
this is a nice way to turn speakers on and off.

[outlets]: https://www.ikea.com/nl/en/p/tradfri-wireless-control-outlet-90356166/

## Controlling outlets with libcoap

We can send _Constrained Application Protocol_ (CoAP) messages to the Trådfri
gateway using `coap-client`, part of [libcoap](https://www.libcoap.net/). You
will need the <abbr>ip</abbr> address of the gateway, and the security code on
the back of the device.

[This excellent guide to Trådfri’s CoAP implementation][coap-docs] explains how
to authenticate, and interact with the gateway. You can also use
[Pytradfri][pytradfri] for a nicer interface to browsing your devices, and to
find the id of your outlets. In this example, we want to control device 65539.

To turn an outlet on, we need to send a message with the following payload:

    {"3312": [{"5850": 1}]}

A value of 0 instead of 1 turns the outlet off again. Putting everything
together, the following command will turn the outlet on:

    $ coap-client -u musium -k «redacted»    \
      coaps://192.168.0.100:5684/15001/65539 \
      -m put -e '{"3312": [{"5850": 1}]}'

Note: In my case, the gateway is at `192.168.1.100`. I also authenticated
previously and created the `musium` user [as described here][gate-auth].

[coap-docs]: https://github.com/glenndehaan/ikea-tradfri-coap-docs
[pytradfri]: https://github.com/home-assistant-libs/pytradfri
[gate-auth]: https://github.com/glenndehaan/ikea-tradfri-coap-docs/blob/68976e6641e4533f9ad51ec724942a0b6c143bce/README.md#authenticate
