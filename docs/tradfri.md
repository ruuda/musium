# Trådfri control


Musium can [run a command](configuration.md#exec_pre_playback_path) before
starting playback and after a period of inactivity. In combination with [Ikea
Trådfri wireless control outlets][outlets], this is a nice way to turn speakers
on and off.

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
      coaps://192.168.1.100:5684/15001/65539 \
      -m put -e '{"3312": [{"5850": 1}]}'

Note: In my case, the gateway is at `192.168.1.100`. I also authenticated
previously and created the `musium` user [as described here][gate-auth].

[coap-docs]: https://github.com/glenndehaan/ikea-tradfri-coap-docs
[pytradfri]: https://github.com/home-assistant-libs/pytradfri
[gate-auth]: https://github.com/glenndehaan/ikea-tradfri-coap-docs/blob/68976e6641e4533f9ad51ec724942a0b6c143bce/README.md#authenticate

## Device groups

If you have multiple outlets in a group — for example, one per speaker — and you
want to turn the entire group on and off, the paths are different. Instead of
15001 to address a single device, we use 15004 to address a group. Furthermore,
the `5850` property that toggles the group is not nested in the `3312` property
like with individual outlets. Putting that together, the following command will
turn group 131078 on:

    $ coap-client -u musium -k «redacted»     \
      coaps://192.168.1.100:5684/15004/131078 \
      -m put -e '{"5850": 1}'

## Pre-playback and post-idle scripts

To call `coap-client` before and after playback, we need to create a
pre-playback and post-idle script, that we can use with the
[`exec_pre_playback_path`](configuration.md#exec_pre_playback_path) setting.
Create `pre_playback.sh`, with the following contents:

    #!/bin/sh
    # Turn the outlet for the speaker on.
    coap-client -u musium -k ...

and replace the `coap-client` command with the one for your outlet, as shown in
the previous section. Create a similar `post_idle.sh` script, with the payload
to turn the outlets off again, and make both scripts executable with `chmod +x`.
Now edit your [config file](configuration.md) to point to these scripts:

    exec_pre_playback_path = /path/to/pre_playback.sh
    exec_post_idle_path = /path/to/post_playback.sh
    idle_timeout_seconds = 180

Make sure to restart Musium to pick up the new configuration.
