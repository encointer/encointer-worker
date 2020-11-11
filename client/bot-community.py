#!/usr/bin/python3
import argparse
import subprocess
import geojson

from math import sqrt, ceil
from random_word import RandomWords
from pyproj import Geod
from shutil import copytree

geoid = Geod(ellps='WGS84')

cli = ["./encointer-client", "-u", "wss://cantillon.encointer.org", "-p", "443", "-U", "wss://substratee03.scs.ch", "-P", "443"]
timeout = ["timeout", "1s"]
MRENCLAVE = "3YM1AH5qdQAsh6BjYqDeYKQbuKgyDgNiSoFmqSUJTYvV"
cli_tail = ["--mrenclave", MRENCLAVE]

NUMBER_OF_LOCATIONS = 10
MAX_POPULATION = 2 * NUMBER_OF_LOCATIONS

def move_point(point, az, dist):
    """ move a point a certain distance [meters] into a direction (azimuth) in [degrees] """

    lng_new, lat_new, return_az = geoid.fwd(point['coordinates'][0], point['coordinates'][1], az, dist)
    return geojson.Point([lng_new, lat_new])

def populate_locations(northwest, n, dist=1000):
    """ populate approximately n locations on a square grid of a specified distance in meters """
    row = [ northwest ]
    for li in range(1, round(sqrt(n))):
        row.append(move_point(row[-1], 90, dist))
    locations = []
    for pnt in row:
        col = [ pnt ]
        for li in range(1, round(sqrt(n))):
            col.append(move_point(col[-1], 180, dist))
        locations += col
    return locations

def next_phase():
    subprocess.run(cli + ["next-phase"])

def get_phase():
    ret = subprocess.run(cli + ["get-phase"], stdout=subprocess.PIPE)
    return ret.stdout.strip().decode("utf-8")

def list_accounts(cid):
    global cli_tail
    ret = subprocess.run(cli + ["trusted", "list-accounts"] + cli_tail, stdout=subprocess.PIPE)
    lines = ret.stdout.decode("utf-8").splitlines() 
    accounts = []
    for line in lines:
        if (not "sr25519" in line) and (not "ed25519" in line):
            accounts.append(line.strip())
    return accounts

def new_account(cid):
    global cli_tail
    ret = subprocess.run(cli + ["trusted", "new-account"] + cli_tail, stdout=subprocess.PIPE)
    return ret.stdout.decode("utf-8").strip()

def faucet(accounts):
    subprocess.run(cli + ["faucet"] + accounts, stdout=subprocess.PIPE)

def balance(accounts, **kwargs):
    bal = []
    for account in accounts:
        if 'cid' in kwargs:
            ret = subprocess.run(cli + ["trusted", "balance", account, "--shard", kwargs.get('cid'), "--mrenclave", MRENCLAVE], stdout=subprocess.PIPE)
        else:
	        ret = subprocess.run(cli + ["balance", account], stdout=subprocess.PIPE)
        print(ret.stdout.strip().decode("utf-8"))
        bal.append(float(ret.stdout.strip().decode("utf-8").split(' ')[-1]))
    return bal

def new_currency(specfile):
    ret = subprocess.run(cli + ["new-currency", specfile, '//Alice'], stdout=subprocess.PIPE)
    return ret.stdout.decode("utf-8").strip()

def await_block():
    subprocess.run(cli + ["listen", "-b", "1"], stdout=subprocess.PIPE)

def register_participant(account, cid):
    global cli_tail
    ret = subprocess.run(cli + ["trusted", "register-participant", account] + cli_tail, stdout=subprocess.PIPE)
    #print(ret.stdout.decode("utf-8"))

def new_claim(account, vote, cid):
    global cli_tail
    ret = subprocess.run(cli + ["trusted", "new-claim", account, str(vote)] + cli_tail, stdout=subprocess.PIPE)
    return ret.stdout.decode("utf-8").strip()

def sign_claim(account, claim, cid):
    global cli_tail
    ret = subprocess.run(cli + ["trusted", "sign-claim", account, claim] + cli_tail, stdout=subprocess.PIPE)
    return ret.stdout.decode("utf-8").strip()

def list_meetups(cid):
    global cli_tail
    accounts = list_accounts(cid)
    meetups = {}
    for account in accounts:
        print("checking assignement for " + account)
        ret = subprocess.run(cli + ["trusted", "get-meetup", account] + cli_tail, stdout=subprocess.PIPE)
        mindex = ret.stdout.decode("utf-8")
        if mindex == '':
            continue
        mindex = int(mindex) 
        print("mindex=" + str(mindex))           
        if not mindex in meetups.keys():
            meetups[mindex] = []
        meetups[mindex].append(account)
    meetuplists = []
    for key, attendees in meetups.items():
        meetuplists.append(attendees)
    print(meetuplists)
    return meetuplists

def register_attestations(account, attestations, cid):
    global cli_tail
    subprocess.run(cli + ["trusted", "register-attestations", account] + attestations + cli_tail, stdout=subprocess.PIPE)


def generate_currency_spec(name, locations, bootstrappers):
    gj = geojson.FeatureCollection(list(map(lambda x : geojson.Feature(geometry=x), locations)))
    gj['currency_meta'] = { 'name': name, 'bootstrappers': bootstrappers }
    fname = name + '.json'
    with open(fname, 'w') as outfile:
        geojson.dump(gj, outfile)
    return fname
    
def random_currency_spec(nloc):
    point = geojson.utils.generate_random("Point", boundingBox=[-56, 41, -21, 13])
    locations = populate_locations(point, NUMBER_OF_LOCATIONS)
    print("created " + str(len(locations)) + " random locations around " + str(point))
    bootstrappers = []
    for bi in range(0,10):
        bootstrappers.append(new_account(MRENCLAVE))
    print('new bootstrappers:' + ' '.join(bootstrappers))
    faucet(bootstrappers)
    await_block()
    name = 'currencyspec' # + '-'.join(RandomWords().get_random_word())
    return generate_currency_spec(name, locations, bootstrappers)

def init():
    print("initializing community")
    specfile = random_currency_spec(16)
    print("generated currency spec: ", specfile)
    cid = new_currency(specfile)
    print("created community with cid: ", cid)
    f = open("cid.txt", "w")
    f.write(cid)
    f.close()
    # now we need t ocopy over our accounts to the new shard
    copytree("./my_trusted_keystore/" + MRENCLAVE, "./my_trusted_keystore/" + cid)

def run():
    global cli_tail
    f = open("cid.txt", "r")
    cid = f.read()
    print("cid is " + cid)
    cli_tail += ["--shard", cid]
    phase = get_phase()
    print("phase is " + phase)
    accounts = list_accounts(cid)
    print("number of known accounts: " + str(len(accounts)))
    if phase == 'REGISTERING':
        bal = balance(accounts, cid=cid)
        total = sum(bal)
        print("****** money supply is " + str(total))
        f = open("bot-stats.csv", "a")
        f.write(str(len(accounts)) + ", " + str(total) + "\n")
        f.close()
        if total > 0:
            n_newbies = min(ceil(len(accounts) / 4.0), MAX_POPULATION - len(accounts))
            print("*** adding " + str(n_newbies) + " newbies")
            if n_newbies > 0:
                newbies = []
                for n in range(0,n_newbies):
                    newbies.append(new_account(cid))
                faucet(newbies)
                await_block()
                accounts = list_accounts(cid)

        print("registering " + str(len(accounts)) + " participants")
        for p in accounts:
            print("registering " + p)
            register_participant(p, cid)
        await_block()
    if phase == 'ASSIGNING':
        meetups = list_meetups(cid)
        print("****** Assigned " + str(len(meetups)) + " meetups")
    if phase == 'ATTESTING':
        meetups = list_meetups(cid)
        print("****** Performing " + str(len(meetups)) + " meetups")
        for meetup in meetups:
            n = len(meetup)
            print("Performing meetup with " + str(n) + " participants")
            claims = {}
            for p in meetup:
                claims[p] = new_claim(p, n, cid)
            print("finished claims")
            for claimant in meetup:
                attestations = []
                for attester in meetup:
                    if claimant == attester:
                        continue
                    print(claimant + " is attested by " + attester)
                    attestations.append(sign_claim(attester, claims[claimant], cid))
                print("registering attestations for " + claimant)
                register_attestations(claimant, attestations, cid)
        await_block()

def benchmark():            
    print("will grow population forever")
    while True:
        run()
        await_block
        next_phase()
        await_block

if __name__ == '__main__':
    parser = argparse.ArgumentParser(prog='bot-community')
    subparsers = parser.add_subparsers(dest='subparser', help='sub-command help')
    parser_a = subparsers.add_parser('init', help='a help')
    parser_b = subparsers.add_parser('run', help='b help')
    parser_c = subparsers.add_parser('benchmark', help='b help')

    kwargs = vars(parser.parse_args())
    try:
        globals()[kwargs.pop('subparser')](**kwargs)
    except KeyError:
        parser.print_help()
