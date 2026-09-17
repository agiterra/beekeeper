#!/usr/bin/env python3
"""Regenerate fixtures/fold-vectors.json from hand-stated expectations.

Every expectation below is written by hand from CONTRACT.md; this script
only assembles the long hex ids and the event envelopes. Run it from the
repo root after editing, then make all three folds pass the result — never
edit the expectation to match one implementation.
"""
import json, os, sys
HERE=os.path.dirname(os.path.abspath(__file__))
P="30621:"+"a"*64+":tank-loop"
OTHER="30621:"+"b"*64+":other"
ALICE="1"*64; BOB="2"*64
L1="1"*32; L2="2"*32; L3="3"*32
I1="a"*32; I2="b"*32; I3="c"*32
SCHEMA="buzz-project-todo/v1"
def eid(n): return f"{n:064x}"
def content(op, listId, **kw):
    d={"schema":SCHEMA,"op":op,"listId":listId}; d.update(kw)
    return json.dumps(d,separators=(",",":"))
def ev(n, pk, t, op, listId, itemId=None, kind=44248, tags=None, content_override=None, vis="project", **kw):
    if itemId is not None: kw={"itemId":itemId, **kw}
    if op=="list.create" and "visibility" not in kw: kw["visibility"]=vis
    c = content_override if content_override is not None else content(op, listId, **kw)
    if tags is None:
        tags=[["a",P],["td-v","td1-1"],["td-op",op],["td-list",listId],["td-vis",vis]]
        if itemId is not None: tags.append(["td-item",itemId])
    return {"id":eid(n),"pubkey":pk,"created_at":t,"kind":kind,"tags":tags,"content":c}
def item(id, listId, text, rank, createdAt, createdBy, updatedAt, done=False, assignee=None, due=None, completedAt=None, completedBy=None):
    return {"id":id,"listId":listId,"text":text,"done":done,"rank":rank,"assignee":assignee,"due":due,
            "createdAt":createdAt,"createdBy":createdBy,"updatedAt":updatedAt,"completedAt":completedAt,"completedBy":completedBy}
def lst(id,title,createdAt,createdBy,updatedAt,open=(),completed=(),archived=False,visibility="project",pinned=False):
    return {"id":id,"title":title,"visibility":visibility,"archived":archived,"pinned":pinned,"createdAt":createdAt,"createdBy":createdBy,"updatedAt":updatedAt,"open":list(open),"completed":list(completed)}
def digest(lists, ignored=0, project=P):
    return {"schema":"buzz-project-todo-digest/v1","project":project,"ignored":ignored,"lists":lists}

cases=[]
cases.append({"name":"empty","project":P,"events":[],"expected":digest([])})

cases.append({"name":"basic list with open and completed items","project":P,"events":[
    ev(1,ALICE,100,"list.create",L1,title="Launch"),
    ev(2,ALICE,101,"item.add",L1,I1,text="Write the NIP",rank="a0"),
    ev(3,BOB,102,"item.add",L1,I2,text="Ship it",rank="a1"),
    ev(4,BOB,110,"item.done",L1,I1,done=True),
],"expected":digest([lst(L1,"Launch",100,ALICE,110,
    open=[item(I2,L1,"Ship it","a1",102,BOB,102)],
    completed=[item(I1,L1,"Write the NIP","a0",101,ALICE,110,done=True,completedAt=110,completedBy=BOB)])])})

cases.append({"name":"per-field last write wins; ties break on event id; a skewed write before the add loses","project":P,"events":[
    ev(1,ALICE,50,"list.create",L1,title="L"),
    ev(2,ALICE,100,"item.add",L1,I1,text="v1",rank="a0"),
    ev(3,BOB,90,"item.text",L1,I1,text="skewed"),
    ev(4,ALICE,105,"item.text",L1,I1,text="v2"),
    ev(5,BOB,105,"item.text",L1,I1,text="v2b"),
    ev(6,BOB,103,"item.assignee",L1,I1,assignee=BOB),
    ev(7,ALICE,104,"item.due",L1,I1,due="2026-10-01"),
    ev(8,ALICE,106,"item.rank",L1,I1,rank="a0V"),
],"expected":digest([lst(L1,"L",50,ALICE,106,
    open=[item(I1,L1,"v2b","a0V",100,ALICE,106,assignee=BOB,due="2026-10-01")])])})

cases.append({"name":"remove is terminal whenever it was stamped; later ops on a removed item are dropped silently","project":P,"events":[
    ev(1,ALICE,1,"list.create",L1,title="L"),
    ev(2,ALICE,2,"item.add",L1,I1,text="one",rank="a0"),
    ev(3,BOB,3,"item.remove",L1,I1),
    ev(4,ALICE,4,"item.text",L1,I1,text="edited after remove"),
    ev(5,ALICE,5,"item.done",L1,I1,done=True),
    ev(6,ALICE,2,"item.add",L1,I2,text="two",rank="a1"),
    ev(7,BOB,1,"item.remove",L1,I2),
],"expected":digest([lst(L1,"L",1,ALICE,3)])})

cases.append({"name":"earliest create wins; ops on missing lists and items are counted as ignored","project":P,"events":[
    ev(1,ALICE,10,"list.create",L1,title="First"),
    ev(2,BOB,5,"list.create",L1,title="Earlier"),
    ev(3,ALICE,11,"item.add",L2,I1,text="orphan",rank="a0"),
    ev(4,ALICE,12,"item.text",L1,I2,text="no such item"),
    ev(5,ALICE,13,"list.title",L2,title="no such list"),
    ev(6,ALICE,14,"list.title",L1,title="Renamed"),
    ev(7,ALICE,15,"item.remove",L1,I3),
],"expected":digest([lst(L1,"Renamed",5,BOB,14)],ignored=5)})

cases.append({"name":"completed sorts most recent first; undo returns the item to its rank","project":P,"events":[
    ev(1,ALICE,1,"list.create",L1,title="L"),
    ev(2,ALICE,2,"item.add",L1,I1,text="one",rank="a0"),
    ev(3,ALICE,3,"item.add",L1,I2,text="two",rank="a1"),
    ev(4,ALICE,4,"item.add",L1,I3,text="three",rank="a2"),
    ev(5,BOB,10,"item.done",L1,I1,done=True),
    ev(6,BOB,20,"item.done",L1,I2,done=True),
    ev(7,ALICE,15,"item.done",L1,I3,done=True),
    ev(8,ALICE,25,"item.done",L1,I1,done=False),
],"expected":digest([lst(L1,"L",1,ALICE,25,
    open=[item(I1,L1,"one","a0",2,ALICE,25)],
    completed=[item(I2,L1,"two","a1",3,ALICE,20,done=True,completedAt=20,completedBy=BOB),
               item(I3,L1,"three","a2",4,ALICE,15,done=True,completedAt=15,completedBy=ALICE)])])})

cases.append({"name":"events that do not decode are ignored; duplicate ids are kept once","project":P,"events":[
    ev(1,ALICE,1,"list.create",L1,title="L"),
    ev(1,ALICE,1,"list.create",L1,title="L"),
    ev(2,ALICE,2,"list.create",L2,title="wrong kind",kind=44240),
    ev(3,ALICE,3,"list.create",L2,title="two a tags",tags=[["a",P],["a",P],["td-v","td1-1"],["td-op","list.create"],["td-list",L2],["td-vis","project"]]),
    ev(4,ALICE,4,"list.create",L2,title="other project",tags=[["a",OTHER],["td-v","td1-1"],["td-op","list.create"],["td-list",L2],["td-vis","project"]]),
    ev(5,ALICE,5,"list.create",L2,content_override="{not json"),
    ev(6,ALICE,6,"list.create",L2,content_override=json.dumps({"schema":SCHEMA,"op":"list.create","listId":L2,"title":"x","visibility":"project","extra":1})),
    ev(7,ALICE,7,"list.create",L2,title="no td-vis tag",tags=[["a",P],["td-v","td1-1"],["td-op","list.create"],["td-list",L2]]),
    ev(8,ALICE,8,"list.create",L2,title="create disagrees with tag",vis="project",content_override=json.dumps({"schema":SCHEMA,"op":"list.create","listId":L2,"title":"x","visibility":"personal"})),
],"expected":digest([lst(L1,"L",1,ALICE,1)],ignored=7)})

cases.append({"name":"archived is reversible; lists order by createdAt then id","project":P,"events":[
    ev(1,ALICE,5,"list.create",L2,title="Second"),
    ev(2,ALICE,5,"list.create",L1,title="First"),
    ev(3,BOB,6,"list.archived",L2,archived=True),
    ev(4,BOB,7,"list.archived",L2,archived=False),
    ev(5,BOB,8,"list.archived",L1,archived=True),
],"expected":digest([lst(L1,"First",5,ALICE,8,archived=True),lst(L2,"Second",5,ALICE,7,archived=False)])})

cases.append({"name":"open items with equal ranks order by id","project":P,"events":[
    ev(1,ALICE,1,"list.create",L1,title="L"),
    ev(2,ALICE,2,"item.add",L1,I2,text="b",rank="a0"),
    ev(3,ALICE,3,"item.add",L1,I1,text="a",rank="a0"),
],"expected":digest([lst(L1,"L",1,ALICE,3,open=[item(I1,L1,"a","a0",3,ALICE,3),item(I2,L1,"b","a0",2,ALICE,2)])])})

cases.append({"name":"pinned is per-field last write wins, shared by every member","project":P,"events":[
    ev(1,ALICE,1,"list.create",L1,title="L"),
    ev(2,BOB,2,"list.pinned",L1,pinned=True),
    ev(3,ALICE,3,"list.pinned",L1,pinned=False),
    ev(4,BOB,3,"list.pinned",L1,pinned=True),
],"expected":digest([lst(L1,"L",1,ALICE,3,pinned=True)])})

cases.append({"name":"a personal list takes ops from its creator only; a visibility tag that disagrees with the list is ignored","project":P,"events":[
    ev(1,ALICE,1,"list.create",L1,title="Mine",vis="personal"),
    ev(2,ALICE,2,"item.add",L1,I1,text="private one",rank="a0",vis="personal"),
    ev(3,BOB,3,"item.add",L1,I2,text="bob intrudes",rank="a1",vis="personal"),
    ev(4,BOB,4,"item.done",L1,I1,done=True,vis="personal"),
    ev(5,ALICE,5,"item.text",L1,I1,text="tagged as project",vis="project"),
    ev(6,ALICE,6,"list.pinned",L1,pinned=True,vis="personal"),
    ev(7,ALICE,7,"list.create",L2,title="Shared"),
    ev(8,ALICE,8,"item.add",L2,I3,text="mis-tagged as personal",rank="a0",vis="personal"),
],"expected":digest([
    lst(L1,"Mine",1,ALICE,6,visibility="personal",pinned=True,open=[item(I1,L1,"private one","a0",2,ALICE,2)]),
    lst(L2,"Shared",7,ALICE,7),
],ignored=4)})

out={"schema":"buzz-project-todo-fold-vectors/v1","cases":cases}
with open(os.path.join(HERE,"fixtures","fold-vectors.json"),"w") as f:
    json.dump(out,f,indent=2); f.write("\n")
print(f"wrote {len(cases)} cases")
